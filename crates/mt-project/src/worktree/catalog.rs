use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use git2::Repository;
use parking_lot::{Condvar, Mutex};

use super::porcelain::{parse_porcelain_text, parse_porcelain_z};
use super::{GitAnnotation, WorktreeFact, WorktreePathState, WorktreeScan, WorktreeScanSource};

const SCAN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
struct RawGitOutput {
    success: bool,
    status_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

trait GitRunner: Send + Sync + 'static {
    fn run(&self, repo_path: &Path, args: &[&str], timeout: Duration) -> Result<RawGitOutput>;
}

struct SystemGitRunner {
    program: OsString,
}

impl Default for SystemGitRunner {
    fn default() -> Self {
        Self {
            program: OsString::from("git"),
        }
    }
}

impl SystemGitRunner {
    #[cfg(test)]
    fn with_program(program: impl AsRef<std::ffi::OsStr>) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
        }
    }
}

impl GitRunner for SystemGitRunner {
    fn run(&self, repo_path: &Path, args: &[&str], timeout: Duration) -> Result<RawGitOutput> {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .current_dir(repo_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        super::super::git::hide_console_window(&mut command);
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("git stdout pipe was not created"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("git stderr pipe was not created"))?;
        let stdout_reader = std::thread::spawn(move || read_pipe(stdout));
        let stderr_reader = std::thread::spawn(move || read_pipe(stderr));
        let started = Instant::now();

        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() >= timeout {
                let kill_error = child.kill().err();
                let wait_error = child.wait().err();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                let cleanup = match (kill_error, wait_error) {
                    (None, None) => String::new(),
                    (kill, wait) => format!("; cleanup errors: kill={kill:?}, wait={wait:?}"),
                };
                return Err(anyhow!(
                    "git worktree list timed out after {}ms{}",
                    timeout.as_millis(),
                    cleanup
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        };

        let stdout = stdout_reader
            .join()
            .map_err(|_| anyhow!("git stdout reader panicked"))??;
        let stderr = stderr_reader
            .join()
            .map_err(|_| anyhow!("git stderr reader panicked"))??;
        Ok(RawGitOutput {
            success: status.success(),
            status_code: status.code(),
            stdout,
            stderr,
        })
    }
}

fn read_pipe(mut pipe: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    pipe.read_to_end(&mut output)?;
    Ok(output)
}

#[derive(Default)]
struct RepoState {
    generation: u64,
    last_authoritative: Option<Vec<WorktreeFact>>,
    flights: HashMap<u64, Arc<Flight>>,
}

struct Flight {
    result: Mutex<Option<Result<WorktreeScan, String>>>,
    ready: Condvar,
}

impl Flight {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    fn wait(&self) -> Result<WorktreeScan, String> {
        let mut result = self.result.lock();
        while result.is_none() {
            self.ready.wait(&mut result);
        }
        result.clone().expect("flight result set before notify")
    }

    fn finish(&self, value: Result<WorktreeScan, String>) {
        *self.result.lock() = Some(value);
        self.ready.notify_all();
    }
}

struct WorktreeCatalog<R: GitRunner> {
    runner: R,
    states: Mutex<HashMap<PathBuf, RepoState>>,
}

impl<R: GitRunner> WorktreeCatalog<R> {
    fn new(runner: R) -> Self {
        Self {
            runner,
            states: Mutex::new(HashMap::new()),
        }
    }

    fn generation(&self, repo_path: &Path) -> u64 {
        let key = repository_key(repo_path);
        self.states
            .lock()
            .get(&key)
            .map(|state| state.generation)
            .unwrap_or(0)
    }

    fn invalidate(&self, repo_path: &Path) {
        let key = repository_key(repo_path);
        let mut states = self.states.lock();
        let state = states.entry(key).or_default();
        state.generation = state.generation.wrapping_add(1);
    }

    fn scan(&self, repo_path: &Path) -> Result<WorktreeScan, String> {
        let key = repository_key(repo_path);
        let (generation, flight, owner) = {
            let mut states = self.states.lock();
            let state = states.entry(key.clone()).or_default();
            let generation = state.generation;
            if let Some(flight) = state.flights.get(&generation) {
                (generation, flight.clone(), false)
            } else {
                let flight = Arc::new(Flight::new());
                state.flights.insert(generation, flight.clone());
                (generation, flight, true)
            }
        };

        if !owner {
            return flight.wait();
        }

        let attempted = self
            .scan_authoritative(repo_path, generation)
            .or_else(|warning| self.degraded(repo_path, &key, generation, warning));

        {
            let mut states = self.states.lock();
            let state = states.entry(key).or_default();
            let result = if state.generation != generation {
                let warning = format!(
                    "worktree scan generation {generation} was invalidated by mutation generation {}",
                    state.generation
                );
                if let Some(last) = &state.last_authoritative {
                    Ok(WorktreeScan {
                        generation: state.generation,
                        source: WorktreeScanSource::LastKnown,
                        authoritative: false,
                        worktrees: last.clone(),
                        warning: Some(warning),
                    })
                } else {
                    attempted.map(|mut scan| {
                        scan.generation = state.generation;
                        scan.source = WorktreeScanSource::LastKnown;
                        scan.authoritative = false;
                        scan.warning = Some(warning);
                        scan
                    })
                }
            } else {
                if let Ok(scan) = &attempted
                    && scan.authoritative
                {
                    state.last_authoritative = Some(scan.worktrees.clone());
                }
                attempted
            };
            flight.finish(result.clone());
            state.flights.remove(&generation);
            result
        }
    }

    fn scan_authoritative(
        &self,
        repo_path: &Path,
        generation: u64,
    ) -> Result<WorktreeScan, String> {
        let nul = self
            .runner
            .run(
                repo_path,
                &["worktree", "list", "--porcelain", "-z"],
                SCAN_TIMEOUT,
            )
            .map_err(|err| format!("failed to run NUL porcelain scan: {err:#}"))?;
        if nul.success {
            let mut worktrees = parse_porcelain_z(&nul.stdout)
                .map_err(|err| format!("invalid NUL porcelain output: {err:#}"))?;
            enrich_paths(&mut worktrees, false);
            return Ok(WorktreeScan {
                generation,
                source: WorktreeScanSource::PorcelainZ,
                authoritative: true,
                worktrees,
                warning: None,
            });
        }

        if nul.status_code != Some(129) {
            return Err(command_failure("NUL porcelain scan", &nul));
        }

        let text = self
            .runner
            .run(
                repo_path,
                &["worktree", "list", "--porcelain"],
                SCAN_TIMEOUT,
            )
            .map_err(|err| format!("failed to run text porcelain scan: {err:#}"))?;
        if !text.success {
            return Err(command_failure("text porcelain scan", &text));
        }
        let mut worktrees = parse_porcelain_text(&text.stdout)
            .map_err(|err| format!("invalid text porcelain output: {err:#}"))?;
        enrich_paths(&mut worktrees, true);
        Ok(WorktreeScan {
            generation,
            source: WorktreeScanSource::PorcelainText,
            authoritative: true,
            worktrees,
            warning: None,
        })
    }

    fn degraded(
        &self,
        repo_path: &Path,
        key: &Path,
        generation: u64,
        warning: String,
    ) -> Result<WorktreeScan, String> {
        if let Some(last) = self
            .states
            .lock()
            .get(key)
            .and_then(|state| state.last_authoritative.clone())
        {
            return Ok(WorktreeScan {
                generation,
                source: WorktreeScanSource::LastKnown,
                authoritative: false,
                worktrees: last,
                warning: Some(warning),
            });
        }

        match libgit2_fallback(repo_path) {
            Ok(worktrees) => Ok(WorktreeScan {
                generation,
                source: WorktreeScanSource::Libgit2Fallback,
                authoritative: false,
                worktrees,
                warning: Some(warning),
            }),
            Err(fallback) => Err(format!(
                "{warning}; libgit2 fallback also failed: {fallback:#}"
            )),
        }
    }
}

fn command_failure(label: &str, output: &RawGitOutput) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!(
        "{label} failed with status {:?}: {}",
        output.status_code,
        stderr.trim()
    )
}

fn repository_key(repo_path: &Path) -> PathBuf {
    let candidate = Repository::open(repo_path)
        .ok()
        .map(|repo| common_git_dir(&repo))
        .unwrap_or_else(|| repo_path.to_path_buf());
    std::fs::canonicalize(&candidate).unwrap_or(candidate)
}

fn common_git_dir(repo: &Repository) -> PathBuf {
    let git_dir = repo.path();
    let Ok(raw) = std::fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let common = Path::new(raw.trim());
    if common.is_absolute() {
        common.to_path_buf()
    } else {
        git_dir.join(common)
    }
}

fn enrich_paths(worktrees: &mut [WorktreeFact], synthesize_prunable: bool) {
    for worktree in worktrees {
        worktree.path_state = match std::fs::metadata(&worktree.path) {
            Ok(_) => WorktreePathState::Present,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => WorktreePathState::Missing,
            Err(_) => WorktreePathState::Unknown,
        };
        if synthesize_prunable
            && !worktree.is_main
            && !worktree.is_bare
            && worktree.locked.is_none()
            && worktree.prunable.is_none()
            && worktree.path_state == WorktreePathState::Missing
        {
            worktree.prunable = Some(GitAnnotation { reason: None });
        }
    }
}

fn libgit2_fallback(repo_path: &Path) -> Result<Vec<WorktreeFact>> {
    let repo = Repository::open(repo_path)?;
    let main_repo = if repo.is_worktree() {
        Repository::open(common_git_dir(&repo))?
    } else {
        repo
    };

    let mut rows = Vec::new();
    if let Some(workdir) = main_repo.workdir() {
        rows.push(fact_from_repo(
            workdir.to_path_buf(),
            &main_repo,
            true,
            false,
        ));
    } else {
        let bare_path = main_repo.path().to_path_buf();
        rows.push(fact_from_repo(bare_path, &main_repo, true, true));
    }

    if let Ok(names) = main_repo.worktrees() {
        for name in names.iter().flatten() {
            let Ok(worktree) = main_repo.find_worktree(name) else {
                continue;
            };
            let path = worktree.path().to_path_buf();
            let repo = Repository::open_from_worktree(&worktree).ok();
            let branch_ref = repo
                .as_ref()
                .and_then(repository_branch_ref)
                .or_else(|| read_registered_branch(main_repo.path(), name));
            let head = repo.as_ref().and_then(repository_head_oid);
            let path_state = if path.exists() {
                WorktreePathState::Present
            } else {
                WorktreePathState::Missing
            };
            rows.push(WorktreeFact {
                path,
                head,
                branch_ref,
                is_main: false,
                is_detached: repo
                    .as_ref()
                    .is_some_and(|repo| repository_branch_ref(repo).is_none()),
                is_bare: false,
                is_sparse: false,
                locked: matches!(
                    worktree.is_locked(),
                    Ok(git2::WorktreeLockStatus::Locked(_))
                )
                .then_some(GitAnnotation { reason: None }),
                prunable: (path_state == WorktreePathState::Missing)
                    .then_some(GitAnnotation { reason: None }),
                path_state,
            });
        }
    }
    Ok(rows)
}

fn fact_from_repo(path: PathBuf, repo: &Repository, is_main: bool, is_bare: bool) -> WorktreeFact {
    let branch_ref = repository_branch_ref(repo);
    WorktreeFact {
        path,
        head: repository_head_oid(repo),
        is_detached: branch_ref.is_none() && repo.head().is_ok(),
        branch_ref,
        is_main,
        is_bare,
        is_sparse: false,
        locked: None,
        prunable: None,
        path_state: WorktreePathState::Present,
    }
}

fn repository_branch_ref(repo: &Repository) -> Option<String> {
    repo.head()
        .ok()
        .filter(|head| head.is_branch())
        .and_then(|head| head.name().map(str::to_string))
}

fn repository_head_oid(repo: &Repository) -> Option<String> {
    repo.head()
        .ok()
        .and_then(|head| head.target())
        .map(|oid| oid.to_string())
}

fn read_registered_branch(main_git_dir: &Path, name: &str) -> Option<String> {
    let head =
        std::fs::read_to_string(main_git_dir.join("worktrees").join(name).join("HEAD")).ok()?;
    head.strip_prefix("ref: ")
        .map(|value| value.trim().to_string())
}

static DEFAULT_CATALOG: std::sync::LazyLock<WorktreeCatalog<SystemGitRunner>> =
    std::sync::LazyLock::new(|| WorktreeCatalog::new(SystemGitRunner::default()));

pub fn scan(repo_path: &Path) -> Result<WorktreeScan> {
    DEFAULT_CATALOG.scan(repo_path).map_err(anyhow::Error::msg)
}

pub fn invalidate(repo_path: &Path) {
    DEFAULT_CATALOG.invalidate(repo_path);
}

pub fn current_generation(repo_path: &Path) -> u64 {
    DEFAULT_CATALOG.generation(repo_path)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[derive(Debug, Clone)]
    struct FakeRunner {
        outputs: Arc<Mutex<VecDeque<RawGitOutput>>>,
        calls: Arc<AtomicUsize>,
        delay: Duration,
    }

    impl FakeRunner {
        fn new(outputs: Vec<RawGitOutput>) -> Self {
            Self {
                outputs: Arc::new(Mutex::new(outputs.into())),
                calls: Arc::new(AtomicUsize::new(0)),
                delay: Duration::ZERO,
            }
        }

        fn delayed(mut self, delay: Duration) -> Self {
            self.delay = delay;
            self
        }
    }

    impl GitRunner for FakeRunner {
        fn run(
            &self,
            _repo_path: &Path,
            _args: &[&str],
            _timeout: Duration,
        ) -> Result<RawGitOutput> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            self.outputs
                .lock()
                .pop_front()
                .ok_or_else(|| anyhow!("no fake output remaining"))
        }
    }

    fn success(stdout: &[u8]) -> RawGitOutput {
        RawGitOutput {
            success: true,
            status_code: Some(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    fn failure(code: i32, stderr: &str) -> RawGitOutput {
        RawGitOutput {
            success: false,
            status_code: Some(code),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn unsupported_nul_mode_falls_back_to_authoritative_text() {
        let runner = FakeRunner::new(vec![
            failure(129, "unknown option z"),
            success(b"worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n"),
        ]);
        let calls = runner.calls.clone();
        let catalog = WorktreeCatalog::new(runner);
        let scan = catalog.scan(Path::new("/repo")).unwrap();
        assert_eq!(scan.source, WorktreeScanSource::PorcelainText);
        assert!(scan.authoritative);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn ordinary_failure_does_not_retry_in_text_mode() {
        let runner = FakeRunner::new(vec![failure(128, "not a repository")]);
        let calls = runner.calls.clone();
        let catalog = WorktreeCatalog::new(runner);
        assert!(catalog.scan(Path::new("/definitely/missing/repo")).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn malformed_refresh_returns_last_authoritative_snapshot() {
        let runner = FakeRunner::new(vec![
            success(b"worktree /repo\0HEAD abc\0branch refs/heads/main\0\0"),
            success(b"HEAD missing-worktree\0\0"),
        ]);
        let catalog = WorktreeCatalog::new(runner);
        let first = catalog.scan(Path::new("/repo")).unwrap();
        assert!(first.authoritative);
        let second = catalog.scan(Path::new("/repo")).unwrap();
        assert!(!second.authoritative);
        assert_eq!(second.source, WorktreeScanSource::LastKnown);
        assert_eq!(second.worktrees, first.worktrees);
    }

    #[test]
    fn concurrent_scans_share_one_flight() {
        let runner = FakeRunner::new(vec![success(
            b"worktree /repo\0HEAD abc\0branch refs/heads/main\0\0",
        )])
        .delayed(Duration::from_millis(100));
        let calls = runner.calls.clone();
        let catalog = Arc::new(WorktreeCatalog::new(runner));
        let mut threads = Vec::new();
        for _ in 0..4 {
            let catalog = catalog.clone();
            threads.push(std::thread::spawn(move || {
                catalog.scan(Path::new("/repo")).unwrap()
            }));
        }
        for thread in threads {
            assert!(thread.join().unwrap().authoritative);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn main_and_linked_paths_share_common_dir_single_flight() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mini-term-common-dir-flight-{nonce}"));
        let repo = root.join("repo");
        let linked = root.join("linked");
        std::fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test"]);
        std::fs::write(repo.join("file.txt"), "one").unwrap();
        run_git(&repo, &["add", "file.txt"]);
        run_git(&repo, &["commit", "-m", "initial"]);
        run_git(
            &repo,
            &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
        );

        let output = format!(
            "worktree {}\0HEAD abc\0branch refs/heads/main\0\0",
            repo.display()
        );
        let runner =
            FakeRunner::new(vec![success(output.as_bytes())]).delayed(Duration::from_millis(100));
        let calls = runner.calls.clone();
        let catalog = Arc::new(WorktreeCatalog::new(runner));
        let barrier = Arc::new(Barrier::new(3));
        let threads: Vec<_> = [repo.clone(), linked.clone()]
            .into_iter()
            .map(|path| {
                let catalog = catalog.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    catalog.scan(&path).unwrap()
                })
            })
            .collect();
        barrier.wait();
        for thread in threads {
            assert!(thread.join().unwrap().authoritative);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn mutation_generation_fences_an_in_flight_scan() {
        let runner = FakeRunner::new(vec![success(
            b"worktree /repo\0HEAD abc\0branch refs/heads/main\0\0",
        )])
        .delayed(Duration::from_millis(100));
        let catalog = Arc::new(WorktreeCatalog::new(runner));
        let scanning = {
            let catalog = catalog.clone();
            std::thread::spawn(move || catalog.scan(Path::new("/repo")).unwrap())
        };
        std::thread::sleep(Duration::from_millis(20));
        catalog.invalidate(Path::new("/repo"));
        let scan = scanning.join().unwrap();
        assert!(!scan.authoritative);
        assert_eq!(scan.generation, 1);
        assert_eq!(catalog.generation(Path::new("/repo")), 1);
    }

    #[test]
    fn text_enrichment_synthesizes_only_eligible_prunable_rows() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mini-term-missing-worktrees-{nonce}"));
        let mut rows: Vec<WorktreeFact> = ["eligible", "main", "bare", "locked", "prunable"]
            .into_iter()
            .map(|name| WorktreeFact {
                path: root.join(name),
                head: Some("abc".into()),
                branch_ref: Some(format!("refs/heads/{name}")),
                is_main: false,
                is_detached: false,
                is_bare: false,
                is_sparse: false,
                locked: None,
                prunable: None,
                path_state: WorktreePathState::Unknown,
            })
            .collect();
        rows[1].is_main = true;
        rows[2].is_bare = true;
        rows[3].locked = Some(GitAnnotation { reason: None });
        rows[4].prunable = Some(GitAnnotation {
            reason: Some("already stale".into()),
        });

        enrich_paths(&mut rows, true);

        assert!(
            rows.iter()
                .all(|row| row.path_state == WorktreePathState::Missing)
        );
        assert_eq!(rows[0].prunable, Some(GitAnnotation { reason: None }));
        assert!(rows[1].prunable.is_none());
        assert!(rows[2].prunable.is_none());
        assert!(rows[3].prunable.is_none());
        assert_eq!(
            rows[4]
                .prunable
                .as_ref()
                .and_then(|marker| marker.reason.as_deref()),
            Some("already stale")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn system_runner_kills_and_waits_for_timed_out_process() {
        use std::os::unix::fs::PermissionsExt;

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mini-term-git-runner-timeout-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        let pid_file = root.join("pid");
        let script = root.join("git-sleeper.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s' $$ > '{}'\nexec sleep 30\n",
                pid_file.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let runner = SystemGitRunner::with_program(&script);
        let error = runner
            .run(&root, &["ignored"], Duration::from_millis(50))
            .unwrap_err()
            .to_string();
        assert!(error.contains("timed out"), "actual error: {error}");
        let pid = std::fs::read_to_string(pid_file).unwrap();
        assert!(
            !Path::new("/proc").join(pid.trim()).exists(),
            "timed-out child process {pid} is still alive"
        );
    }

    #[test]
    fn real_git_smoke_test_covers_linked_detached_locked_and_prunable() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mini-term-worktree-catalog-{nonce}"));
        let repo = root.join("repo");
        let linked = root.join("linked");
        let detached = root.join("detached");
        let stale = root.join("stale");
        std::fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test"]);
        std::fs::write(repo.join("file.txt"), "one").unwrap();
        run_git(&repo, &["add", "file.txt"]);
        run_git(&repo, &["commit", "-m", "initial"]);
        run_git(
            &repo,
            &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
        );
        run_git(
            &repo,
            &["worktree", "add", "--detach", detached.to_str().unwrap()],
        );
        run_git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "busy",
                linked.to_str().unwrap(),
            ],
        );
        run_git(
            &repo,
            &["worktree", "add", "-b", "stale", stale.to_str().unwrap()],
        );
        std::fs::remove_dir_all(&stale).unwrap();

        assert_eq!(repository_key(&repo), repository_key(&linked));
        let catalog = WorktreeCatalog::new(SystemGitRunner::default());
        let scan = catalog.scan(&repo).unwrap();
        assert!(scan.authoritative);
        assert!(scan.worktrees.first().is_some_and(|row| row.is_main));
        assert!(scan.worktrees.iter().any(|row| {
            row.path == linked
                && row.branch_ref.as_deref() == Some("refs/heads/feature")
                && row
                    .locked
                    .as_ref()
                    .and_then(|locked| locked.reason.as_deref())
                    == Some("busy")
        }));
        assert!(
            scan.worktrees
                .iter()
                .any(|row| row.path == detached && row.is_detached)
        );
        assert!(
            scan.worktrees
                .iter()
                .any(|row| row.path == stale && row.prunable.is_some())
        );
        catalog.invalidate(&linked);
        assert_eq!(catalog.generation(&repo), 1);
        assert_eq!(catalog.generation(&linked), 1);
        std::fs::remove_dir_all(root).ok();
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
