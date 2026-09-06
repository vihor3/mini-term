use std::collections::{BTreeMap, BTreeSet};

use mt_github::CommandOutput;
use mt_identity::{ExecutionHostId, HostInstallId, RepoId, WorktreeId};
use parking_lot::Mutex;

use super::host::{Attempt, Dispatch, process_dispatch};
use super::*;

const OID: &str = "1111111111111111111111111111111111111111";
const OTHER: &str = "2222222222222222222222222222222222222222";

fn snapshot(path: &str, backend: ExecutionBackend) -> ProjectExecutionSnapshot {
    let host = ExecutionHostId::derive("git-adapter-fixture", &HostInstallId::new());
    let repo = RepoId::derive(&host, path);
    ProjectExecutionSnapshot {
        project_id: "fixture-project".into(),
        root_project_id: "fixture-root".into(),
        worktree_id: WorktreeId::derive(&repo, path, None),
        execution_host_id: host,
        canonical_path: path.to_owned(),
        root_source_path: path.to_owned(),
        backend,
        host_label: "fixture".into(),
    }
}

fn output(bytes: Vec<u8>, dispatch: Dispatch, code: Option<i32>) -> Attempt {
    Attempt {
        dispatch,
        error: None,
        output: Some(CommandOutput {
            stdout: bytes,
            stderr: Vec::new(),
            exit_code: code,
            timed_out: dispatch == Dispatch::Uncertain,
            stdout_truncated: false,
            stderr_truncated: false,
        }),
    }
}

#[derive(Default)]
struct FakeState {
    commands: Vec<Vec<String>>,
    files: BTreeMap<String, String>,
    mutation_calls: usize,
    mutation: Option<(Dispatch, Option<i32>)>,
    mutation_output: Option<CommandOutput>,
    invalidate_on_dispatch: Option<GitLifetime>,
    malformed_status: bool,
    changed_authority: bool,
    resolver_repositories: Option<BTreeSet<String>>,
    resolver_directories: BTreeSet<String>,
}

struct FakeHost {
    snapshot: ProjectExecutionSnapshot,
    state: Arc<Mutex<FakeState>>,
}

impl Host for FakeHost {
    fn snapshot(&self) -> &ProjectExecutionSnapshot {
        &self.snapshot
    }
    fn run(&self, cwd: &str, plan: &GitCommand) -> Attempt {
        let mut state = self.state.lock();
        state.commands.push(plan.args.clone());
        if plan.effect == cli::CommandEffect::Mutation {
            state.mutation_calls += 1;
            if let Some(lifetime) = &state.invalidate_on_dispatch {
                lifetime.invalidate();
            }
            if let Some(output) = state.mutation_output.clone() {
                return Attempt {
                    dispatch: process_dispatch(&self.snapshot.backend, &output),
                    output: Some(output),
                    error: None,
                };
            }
            let (dispatch, code) = state.mutation.unwrap_or((Dispatch::Completed, Some(0)));
            return output(Vec::new(), dispatch, code);
        }
        let has = |arg| plan.args.iter().any(|argument| argument == arg);
        let root = if let Some(repositories) = &state.resolver_repositories {
            assert!(
                repositories.contains(cwd),
                "resolver sent Git to a non-repository"
            );
            cwd
        } else {
            &self.snapshot.canonical_path
        };
        let bytes = if has("--show-toplevel") {
            format!("{root}\n").into_bytes()
        } else if has("--absolute-git-dir") || has("--git-common-dir") {
            format!(
                "{root}/{}\n",
                if state.changed_authority {
                    ".other"
                } else {
                    ".git"
                }
            )
            .into_bytes()
        } else if has("status") {
            if state.malformed_status {
                b"# branch.oid (initial)\0".to_vec()
            } else {
                let mut bytes = format!("# branch.oid {OID}\0# branch.head main\0").into_bytes();
                for name in state.files.keys() {
                    bytes.extend_from_slice(format!("? {name}\0").as_bytes());
                }
                bytes
            }
        } else if has("ls-files") {
            Vec::new()
        } else if has("ls-tree") {
            let path = plan.args.last().unwrap();
            assert!(state.files.contains_key(path), "unknown fake worktree file");
            assert_eq!(
                plan,
                &cli::tree_entry_plan(&ObjectId::parse(OID).unwrap(), path).unwrap()
            );
            // Fake files are all untracked, absent from both HEAD and the index.
            Vec::new()
        } else if has("hash-object") {
            format!("{}\n", state.files.get(plan.args.last().unwrap()).unwrap()).into_bytes()
        } else if has("for-each-ref") {
            format!("refs/heads/main\0{OID}\0\0\n").into_bytes()
        } else if has("worktree") {
            format!("worktree {root}\0HEAD {OID}\0branch refs/heads/main\0\0").into_bytes()
        } else {
            panic!("unexpected read plan: {:?}", plan.args);
        };
        output(bytes, Dispatch::Completed, Some(0))
    }
    fn canonical_directory(&self, path: &str) -> GitResult<String> {
        Ok(path.to_owned())
    }
    fn kind(&self, path: &str) -> GitResult<Option<NodeKind>> {
        let state = self.state.lock();
        let Some(repositories) = &state.resolver_repositories else {
            return Ok(Some(NodeKind::Directory));
        };
        Ok((state.resolver_directories.contains(path)
            || path
                .strip_suffix("/.git")
                .is_some_and(|root| repositories.contains(root)))
        .then_some(NodeKind::Directory))
    }
    fn directories(&self, _: &str) -> GitResult<Vec<String>> {
        assert!(
            self.state.lock().resolver_repositories.is_none(),
            "file resolver must not scan descendants"
        );
        Ok(Vec::new())
    }
    fn directory_empty(&self, _: &str) -> GitResult<bool> {
        Ok(true)
    }
    fn file_kind(&self, _: &str, path: &str) -> GitResult<Option<NodeKind>> {
        Ok(self
            .state
            .lock()
            .files
            .contains_key(path)
            .then_some(NodeKind::File))
    }
    fn read_file(&self, _: &str, path: &str) -> GitResult<OwnedContent> {
        Ok(self
            .state
            .lock()
            .files
            .get(path)
            .map(|text| OwnedContent::Bytes(text.as_bytes().to_vec()))
            .unwrap_or(OwnedContent::Missing))
    }
    fn remove_file(&self, _: &str, path: &str) -> Attempt {
        let mut state = self.state.lock();
        state.mutation_calls += 1;
        state.files.remove(path);
        output(Vec::new(), Dispatch::Completed, Some(0))
    }
}

fn fake() -> (GitRepository, Arc<Mutex<FakeState>>) {
    let root = format!("/git-fixture-{}", next_id().unwrap());
    let snapshot = snapshot(
        &root,
        ExecutionBackend::Wsl {
            distro: "fake-only".into(),
        },
    );
    let state = Arc::new(Mutex::new(FakeState::default()));
    let backend = GitBackend {
        host: Arc::new(FakeHost {
            snapshot,
            state: state.clone(),
        }),
        lifetime: GitLifetime::new(),
        anchor: root.clone(),
        probe_anchor: root.clone(),
        recovery_only: false,
    };
    (backend.repository_at(&root).unwrap(), state)
}

#[test]
fn process_dispatch_distinguishes_wsl_launcher_loss_from_native_and_git_exits() {
    for (backend, launcher_loss) in [
        (ExecutionBackend::Local, Dispatch::Completed),
        (
            ExecutionBackend::Wsl {
                distro: "fake-only".into(),
            },
            Dispatch::Uncertain,
        ),
    ] {
        for (exit_code, timed_out, expected) in [
            (Some(-1), false, launcher_loss),
            (Some(-2), false, Dispatch::Completed),
            (Some(0), false, Dispatch::Completed),
            (Some(1), false, Dispatch::Completed),
            (Some(128), false, Dispatch::Completed),
            (Some(255), false, Dispatch::Completed),
            (Some(-1), true, Dispatch::Uncertain),
            (Some(0), true, Dispatch::Uncertain),
            (Some(1), true, Dispatch::Uncertain),
            (None, false, Dispatch::Uncertain),
            (None, true, Dispatch::Uncertain),
        ] {
            let output = CommandOutput {
                stdout: b"captured stdout\n".to_vec(),
                stderr: b"captured stderr\n".to_vec(),
                exit_code,
                timed_out,
                stdout_truncated: false,
                stderr_truncated: false,
            };
            assert_eq!(
                process_dispatch(&backend, &output),
                expected,
                "{backend:?}, exit {exit_code:?}, timed_out {timed_out}"
            );
        }
    }
}

#[test]
fn readiness_epoch_bootstraps_none_but_never_replaces_a_captured_epoch() {
    assert_eq!(GitBackend::checked_readiness_epoch(None, 7), Ok(7));
    assert_eq!(GitBackend::checked_readiness_epoch(None, 8), Ok(8));
    assert_eq!(GitBackend::checked_readiness_epoch(Some(7), 7), Ok(7));
    assert_eq!(GitBackend::checked_readiness_epoch(Some(8), 8), Ok(8));
    assert_eq!(
        GitBackend::checked_readiness_epoch(Some(7), 8),
        Err(GitError::stale())
    );
    assert_eq!(
        GitBackend::checked_readiness_epoch(Some(8), 7),
        Err(GitError::stale())
    );
}

#[test]
fn source_lifetime_and_request_id_fence_aba_publication() {
    let (repository, _) = fake();
    let request = repository.request(GitRead::Status).unwrap();
    let id = request.id();
    let result = request.execute().unwrap();
    assert!(result.is_current(&repository, id));
    assert!(!result.is_current(&repository, id + 1));
    let mut reopened = repository.clone();
    reopened.backend.lifetime = GitLifetime::new();
    assert!(!result.is_current(&reopened, id));
    repository.backend.lifetime.invalidate();
    assert!(!result.is_current(&repository, id));
}

#[test]
fn queued_write_rejects_same_status_changed_bytes_without_dispatch() {
    let (repository, state) = fake();
    let path = "line\n:literal[1]";
    state.lock().files.insert(path.into(), OID.into());
    let before = repository.command(&cli::status_plan()).unwrap();
    let prepared = repository
        .prepare_write(GitWrite::Discard {
            paths: vec![path.into()],
        })
        .unwrap();
    let lookup = cli::tree_entry_plan(&ObjectId::parse(OID).unwrap(), path).unwrap();
    assert!(state.lock().commands.contains(&lookup.args));
    state.lock().files.insert(path.into(), OTHER.into());
    assert_eq!(repository.command(&cli::status_plan()).unwrap(), before);
    let outcome = prepared.execute();
    assert_eq!(outcome.state, GitWriteState::NotDispatched);
    assert_eq!(outcome.error.unwrap().kind, GitErrorKind::Changed);
    {
        let state = state.lock();
        assert_eq!(state.files.len(), 1);
        assert_eq!(state.files.get(path).map(String::as_str), Some(OTHER));
        assert_eq!(state.mutation_calls, 0);
    }
    assert!(repository.busy().is_none());
}

#[test]
fn queued_source_cancellation_and_changed_common_directory_prevent_dispatch() {
    let (repository, state) = fake();
    let prepared = repository
        .prepare_write(GitWrite::Commit {
            message: "fixture".into(),
        })
        .unwrap();
    state.lock().changed_authority = true;
    assert_eq!(prepared.execute().state, GitWriteState::NotDispatched);
    state.lock().changed_authority = false;
    let prepared = repository.prepare_write(GitWrite::Push).unwrap();
    repository.backend.lifetime.invalidate();
    assert_eq!(prepared.execute().state, GitWriteState::NotDispatched);
    assert!(repository.busy().is_none());
}

#[test]
fn uncertain_write_outlives_view_and_only_its_id_can_release_quarantine() {
    let (repository, state) = fake();
    let prepared = repository
        .prepare_write(GitWrite::Commit {
            message: "one attempt".into(),
        })
        .unwrap();
    let id = prepared.id();
    state.lock().mutation = Some((Dispatch::Uncertain, None));
    state.lock().invalidate_on_dispatch = Some(repository.backend.lifetime());
    let outcome = prepared.execute();
    assert_eq!(outcome.state, GitWriteState::Uncertain);
    assert!(outcome.lease_retained);
    assert!(outcome.reconciliation.is_some());
    assert_eq!(repository.busy().unwrap().operation_id, id);
    let mut linked = repository.clone();
    linked.authority.worktree_root.push_str("/linked");
    linked.authority.git_dir.push_str("/worktrees/linked");
    linked.backend.lifetime = GitLifetime::new();
    assert_eq!(linked.busy().unwrap().operation_id, id);
    let review = UncertainReview::UserConfirmedOriginalOperationStopped;
    assert!(repository.review_uncertain(id + 1, review).is_err());
    assert!(repository.busy().is_some());
    repository.review_uncertain(id, review).unwrap();
    assert!(repository.busy().is_none());
    assert_eq!(
        state
            .lock()
            .commands
            .iter()
            .filter(|args| args.iter().any(|arg| arg == "commit"))
            .count(),
        1
    );
}

#[test]
fn wsl_launcher_loss_retains_exact_lease_after_reconciliation_until_explicit_review() {
    let (repository, state) = fake();
    let prepared = repository
        .prepare_write(GitWrite::Commit {
            message: "one WSL attempt".into(),
        })
        .unwrap();
    let id = prepared.id();
    let queued = repository.prepare_write(GitWrite::Push).unwrap();
    let queued_id = queued.id();
    let source = repository.backend.snapshot().source_signature();
    state.lock().mutation_output = Some(CommandOutput {
        stdout: b"captured mutation output\n".to_vec(),
        stderr: b"The Windows Subsystem for Linux instance has terminated.\r\r\n".to_vec(),
        exit_code: Some(-1),
        timed_out: false,
        stdout_truncated: false,
        stderr_truncated: false,
    });

    let outcome = prepared.execute();
    assert_eq!(outcome.operation_id, id);
    assert_eq!(outcome.state, GitWriteState::Uncertain);
    assert!(!outcome.succeeded());
    assert_eq!(outcome.error.as_ref().unwrap().kind, GitErrorKind::Unavailable);
    assert!(outcome.lease_retained);
    assert!(outcome.reconciliation_error.is_none());
    let reconciliation = outcome.reconciliation.as_ref().unwrap();
    assert!(matches!(reconciliation.postcondition, GitPostcondition::Refreshed));
    assert_eq!(reconciliation.repository.authority(), repository.authority());
    assert_eq!(
        reconciliation.repository.backend.snapshot().source_signature(),
        source
    );
    let busy = repository.busy().unwrap();
    assert_eq!(busy.operation_id, id);
    assert_eq!(busy.phase, GitWritePhase::Uncertain);
    assert_eq!(busy.source, source);
    assert_eq!(busy.project_id, repository.backend.snapshot().project_id);
    assert_eq!(busy.repository, *repository.authority());

    let commands_after_reconciliation = state.lock().commands.len();
    let blocked = queued.execute();
    assert_eq!(blocked.state, GitWriteState::NotDispatched);
    assert_eq!(blocked.error.unwrap().kind, GitErrorKind::Busy);
    assert_eq!(
        repository.prepare_write(GitWrite::Pull).err().unwrap().kind,
        GitErrorKind::Busy
    );
    let review = UncertainReview::UserConfirmedOriginalOperationStopped;
    assert_eq!(
        repository.review_uncertain(queued_id, review).err().unwrap().kind,
        GitErrorKind::Stale
    );
    assert_eq!(state.lock().commands.len(), commands_after_reconciliation);
    assert_eq!(repository.busy().unwrap().operation_id, id);

    state.lock().changed_authority = true;
    assert!(repository.review_uncertain(id, review).is_err());
    assert_eq!(repository.busy().unwrap().operation_id, id);
    assert_eq!(repository.busy().unwrap().phase, GitWritePhase::Uncertain);
    state.lock().changed_authority = false;
    let reviewed = repository.review_uncertain(id, review).unwrap();
    assert!(matches!(reviewed.postcondition, GitPostcondition::Refreshed));
    assert_eq!(reviewed.repository.authority(), repository.authority());
    assert!(repository.busy().is_none());
    assert!(repository.review_uncertain(id, review).is_err());
    let state = state.lock();
    assert_eq!(state.mutation_calls, 1);
    assert_eq!(
        state
            .commands
            .iter()
            .filter(|args| args.iter().any(|arg| arg == "commit"))
            .count(),
        1
    );
}

#[test]
fn known_nonzero_reconciles_original_source_and_releases_its_lease() {
    let (repository, state) = fake();
    let prepared = repository.prepare_write(GitWrite::Pull).unwrap();
    state.lock().mutation = Some((Dispatch::Completed, Some(1)));
    let outcome = prepared.execute();
    assert_eq!(outcome.state, GitWriteState::Completed);
    assert!(!outcome.succeeded());
    assert!(outcome.reconciliation.is_some());
    assert!(repository.busy().is_none());
}

#[test]
fn malformed_capture_is_an_error_not_a_clean_repository() {
    let (repository, state) = fake();
    state.lock().malformed_status = true;
    assert!(
        repository
            .request(GitRead::Status)
            .unwrap()
            .execute()
            .is_err()
    );
    assert!(repository.prepare_write(GitWrite::StageAll).is_err());
}

#[test]
fn bounded_capture_rejects_both_streams_and_unconfirmed_completion() {
    let plan = cli::status_plan();
    let mut attempt = output(Vec::new(), Dispatch::Completed, Some(0));
    attempt.output.as_mut().unwrap().stderr_truncated = true;
    assert!(attempt.checked(&plan).is_err());
    assert!(
        output(Vec::new(), Dispatch::Uncertain, None)
            .checked(&plan)
            .is_err()
    );
    assert!(
        output(Vec::new(), Dispatch::NotDispatched, None)
            .checked(&plan)
            .is_err()
    );
}

#[test]
fn file_resolver_obeys_nearest_repository_deleted_parents_and_ancestor_ceiling() {
    let root = "/source/a/b/c/d/e/f";
    let state = Arc::new(Mutex::new(FakeState {
        resolver_repositories: Some(
            [root.to_owned(), format!("{root}/nested")]
                .into_iter()
                .collect(),
        ),
        resolver_directories: [
            "/source",
            "/source/a",
            "/source/a/b",
            "/source/a/b/c",
            "/source/a/b/c/d",
            "/source/a/b/c/d/e",
            root,
            "/source/a/b/c/d/e/f/nested",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        ..FakeState::default()
    }));
    let backend = GitBackend {
        host: Arc::new(FakeHost {
            snapshot: snapshot(
                root,
                ExecutionBackend::Wsl {
                    distro: "fake-only".into(),
                },
            ),
            state: state.clone(),
        }),
        lifetime: GitLifetime::new(),
        anchor: root.into(),
        probe_anchor: root.into(),
        recovery_only: false,
    };
    let (repository, relative) = backend
        .repository_for_file("nested/deleted/parent/:literal\\name\n")
        .unwrap();
    assert_eq!(
        repository.authority().worktree_root,
        format!("{root}/nested")
    );
    assert_eq!(relative, "deleted/parent/:literal\\name\n");
    assert!(repository.matches_snapshot(backend.snapshot()));
    state.lock().resolver_repositories = Some(["/source".to_owned()].into_iter().collect());
    assert_eq!(
        backend.repository_for_file("file").err().unwrap().kind,
        GitErrorKind::Unavailable
    );
    state.lock().resolver_repositories = Some(["/source/a".to_owned()].into_iter().collect());
    let (repository, relative) = backend.repository_for_file("file").unwrap();
    assert_eq!(repository.authority().worktree_root, "/source/a");
    assert_eq!(relative, "b/c/d/e/f/file");
    state.lock().resolver_repositories = Some(BTreeSet::new());
    assert_eq!(
        backend.repository_for_file("file").err().unwrap().kind,
        GitErrorKind::Unavailable
    );
    state.lock().commands.clear();
    for path in [
        "../escape",
        "/absolute",
        "a/../escape",
        "a//file",
        "directory/",
        ".git/config",
    ] {
        assert!(backend.repository_for_file(path).is_err());
    }
    assert!(state.lock().commands.is_empty());
    backend.lifetime.invalidate();
    assert!(backend.repository_for_file("file").is_err());
}

#[cfg(unix)]
mod actions_fixtures {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Fixture {
        root: PathBuf,
        snapshot: ProjectExecutionSnapshot,
    }
    impl Fixture {
        fn new(parent: &Path) -> Self {
            let root = parent.join(format!(
                "mt-git-adapter-{}-{}",
                std::process::id(),
                next_id().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let root = std::fs::canonicalize(root).unwrap();
            let snapshot = snapshot(root.to_str().unwrap(), ExecutionBackend::Local);
            let fixture = Self { root, snapshot };
            fixture.git(&["init", "--initial-branch=main", "."]);
            fixture.git(&["config", "user.name", "Actions Fixture"]);
            fixture.git(&["config", "user.email", "fixture@example.invalid"]);
            fixture.git(&["config", "commit.gpgsign", "false"]);
            fixture
        }
        fn git(&self, args: &[&str]) -> Vec<u8> {
            let plan = host::auxiliary("git", args, cli::MAX_LIST_BYTES);
            ExecutionGitHost(self.snapshot.clone())
                .run(self.root.to_str().unwrap(), &plan)
                .checked(&plan)
                .unwrap()
        }
        fn repository(&self) -> GitRepository {
            GitBackend::connect(self.snapshot.clone(), GitLifetime::new())
                .unwrap()
                .discover()
                .unwrap()
                .remove(0)
        }
        fn write(&self, name: &str, bytes: &[u8]) {
            std::fs::write(self.root.join(name), bytes).unwrap();
        }
        fn seed(&self) {
            self.write("tracked", b"head\n");
            self.git(&["add", "--", "tracked"]);
            self.git(&["commit", "-m", "root"]);
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    struct NoCommands(ExecutionGitHost);
    impl Host for NoCommands {
        fn snapshot(&self) -> &ProjectExecutionSnapshot {
            self.0.snapshot()
        }
        fn run(&self, _: &str, _: &GitCommand) -> Attempt {
            panic!("native read attempted a host command")
        }
        fn canonical_directory(&self, path: &str) -> GitResult<String> {
            self.0.canonical_directory(path)
        }
        fn kind(&self, path: &str) -> GitResult<Option<NodeKind>> {
            self.0.kind(path)
        }
        fn directories(&self, path: &str) -> GitResult<Vec<String>> {
            self.0.directories(path)
        }
        fn directory_empty(&self, path: &str) -> GitResult<bool> {
            self.0.directory_empty(path)
        }
        fn file_kind(&self, root: &str, path: &str) -> GitResult<Option<NodeKind>> {
            self.0.file_kind(root, path)
        }
        fn read_file(&self, root: &str, path: &str) -> GitResult<OwnedContent> {
            self.0.read_file(root, path)
        }
        fn remove_file(&self, _: &str, _: &str) -> Attempt {
            panic!("native read attempted file removal")
        }
    }

    fn value(repository: &GitRepository, read: GitRead) -> GitReadValue {
        repository.request(read).unwrap().execute().unwrap().value
    }
    fn write(repository: &GitRepository, operation: GitWrite) -> GitWriteOutcome {
        let outcome = repository.prepare_write(operation).unwrap().execute();
        assert!(
            outcome.succeeded(),
            "write failed: {:?}; reconciliation: {:?}",
            outcome.error,
            outcome.reconciliation_error
        );
        outcome
    }

    #[test]
    fn actual_native_read_adapter_never_requires_a_host_git_command() {
        let fixture = Fixture::new(&std::env::temp_dir());
        fixture.seed();
        let root = fixture.root.to_str().unwrap().to_owned();
        let backend = GitBackend {
            host: Arc::new(NoCommands(ExecutionGitHost(fixture.snapshot.clone()))),
            lifetime: GitLifetime::new(),
            anchor: root.clone(),
            probe_anchor: root,
            recovery_only: false,
        };
        let repository = backend.discover().unwrap().remove(0);
        let GitReadValue::Status(status) = value(&repository, GitRead::Status) else {
            panic!()
        };
        let commit = status.head.oid.unwrap();
        value(&repository, GitRead::Branches);
        value(
            &repository,
            GitRead::History {
                before: None,
                branch: status.head.branch,
                limit: 30,
            },
        );
        value(
            &repository,
            GitRead::CommitFiles {
                commit: commit.clone(),
            },
        );
        value(
            &repository,
            GitRead::CommitDiff {
                commit,
                path: "tracked".into(),
                old_path: None,
            },
        );
        value(
            &repository,
            GitRead::WorkingDiff {
                path: "tracked".into(),
                old_path: None,
                staged: true,
            },
        );
        value(
            &repository,
            GitRead::WorkingDiff {
                path: "tracked".into(),
                old_path: None,
                staged: false,
            },
        );
    }

    #[test]
    fn actual_native_file_resolver_keeps_nested_deleted_and_working_head_semantics() {
        let fixture = Fixture::new(&std::env::temp_dir());
        fixture.seed();
        let nested = Fixture::new(&fixture.root);
        nested.seed();
        std::fs::create_dir_all(nested.root.join("deleted/parent")).unwrap();
        nested.write("deleted/parent/file", b"deleted head\n");
        nested.git(&["add", "--", "deleted/parent/file"]);
        nested.git(&["commit", "-m", "deleted parent baseline"]);
        std::fs::remove_dir_all(nested.root.join("deleted")).unwrap();
        nested.write("tracked", b"index\n");
        nested.git(&["add", "--", "tracked"]);
        nested.write("tracked", b"working\n");
        let root = fixture.root.to_str().unwrap().to_owned();
        let backend = GitBackend {
            host: Arc::new(NoCommands(ExecutionGitHost(fixture.snapshot.clone()))),
            lifetime: GitLifetime::new(),
            anchor: root.clone(),
            probe_anchor: root.clone(),
            recovery_only: false,
        };
        let prefix = nested.root.file_name().unwrap().to_str().unwrap();
        let file = format!("{prefix}/tracked");
        let (repository, relative) = backend.repository_for_file(&file).unwrap();
        assert_eq!(
            repository.authority().worktree_root,
            nested.root.to_str().unwrap()
        );
        assert_eq!(relative, "tracked");
        let GitReadValue::Diff(diff) = value(
            &repository,
            GitRead::WorkingDiff {
                path: relative,
                old_path: None,
                staged: false,
            },
        ) else {
            panic!()
        };
        assert_eq!(diff.old_content, "head\n");
        assert_eq!(diff.new_content, "working\n");
        assert_eq!(
            serde_json::to_value(&diff).unwrap(),
            serde_json::to_value(git::get_git_diff(&fixture.root, &file, Some(false)).unwrap())
                .unwrap()
        );
        let (repository, relative) = backend
            .repository_for_file(&format!("{prefix}/deleted/parent/file"))
            .unwrap();
        assert_eq!(relative, "deleted/parent/file");
        let GitReadValue::Diff(diff) = value(
            &repository,
            GitRead::WorkingDiff {
                path: relative,
                old_path: None,
                staged: false,
            },
        ) else {
            panic!()
        };
        assert_eq!(diff.old_content, "deleted head\n");
        assert!(diff.new_content.is_empty());
        symlink(&nested.root, fixture.root.join("alias")).unwrap();
        assert!(backend.repository_for_file("alias/tracked").is_err());

        // The identical path exists locally, but the captured remote host has
        // no repository there. Resolving must never borrow the local answer.
        let state = Arc::new(Mutex::new(FakeState {
            resolver_repositories: Some(BTreeSet::new()),
            resolver_directories: [root.clone()].into_iter().collect(),
            ..FakeState::default()
        }));
        let remote = GitBackend {
            host: Arc::new(FakeHost {
                snapshot: snapshot(
                    &root,
                    ExecutionBackend::Wsl {
                        distro: "fake-only".into(),
                    },
                ),
                state: state.clone(),
            }),
            lifetime: GitLifetime::new(),
            anchor: root.clone(),
            probe_anchor: root,
            recovery_only: false,
        };
        assert!(remote.repository_for_file(&file).is_err());
        assert!(state.lock().commands.is_empty());
    }

    #[test]
    fn actual_detached_native_discovery_preserves_existing_repository_label() {
        let fixture = Fixture::new(&std::env::temp_dir());
        fixture.seed();
        fixture.git(&["checkout", "--detach", "HEAD"]);
        let repository = fixture.repository();
        let existing = git::discover_git_repos(&fixture.root).unwrap();
        let expected = existing
            .iter()
            .find(|row| row.path == fixture.root)
            .unwrap();
        assert!(
            expected
                .current_branch
                .as_ref()
                .is_some_and(|label| label.starts_with('('))
        );
        assert_eq!(repository.info().current_branch, expected.current_branch);
    }

    #[test]
    fn actual_local_unborn_literal_stage_unstage_discard_and_diff_parity() {
        let fixture = Fixture::new(&std::env::temp_dir());
        let repository = fixture.repository();
        let hostile = ":(glob)* line\nname";
        fixture.write(hostile, b"literal\n");
        fixture.write("other", b"untouched\n");
        write(
            &repository,
            GitWrite::Stage {
                paths: vec![hostile.into()],
            },
        );
        let GitReadValue::Status(status) = value(&repository, GitRead::Status) else {
            panic!()
        };
        assert!(status.head.oid.is_none());
        assert!(
            status
                .changes
                .iter()
                .find(|row| row.path == hostile)
                .unwrap()
                .staged_status
                .is_some()
        );
        assert!(
            status
                .changes
                .iter()
                .find(|row| row.path == "other")
                .unwrap()
                .staged_status
                .is_none()
        );
        write(&repository, GitWrite::UnstageAll);
        assert_eq!(
            std::fs::read(fixture.root.join(hostile)).unwrap(),
            b"literal\n"
        );
        write(
            &repository,
            GitWrite::Discard {
                paths: vec![hostile.into()],
            },
        );
        assert!(!fixture.root.join(hostile).exists());
        write(&repository, GitWrite::StageAll);
        write(
            &repository,
            GitWrite::Commit {
                message: "root".into(),
            },
        );
        fixture.write("other", b"working\n");
        let GitReadValue::Diff(diff) = value(
            &repository,
            GitRead::WorkingDiff {
                path: "other".into(),
                old_path: None,
                staged: false,
            },
        ) else {
            panic!()
        };
        let local = git::get_git_diff(&fixture.root, "other", Some(false)).unwrap();
        assert_eq!(
            serde_json::to_value(diff).unwrap(),
            serde_json::to_value(local).unwrap()
        );
    }

    #[test]
    fn actual_local_deleted_binary_limits_and_symlink_parent_are_explicit() {
        let fixture = Fixture::new(&std::env::temp_dir());
        fixture.seed();
        let repository = fixture.repository();
        std::fs::remove_file(fixture.root.join("tracked")).unwrap();
        let GitReadValue::Diff(diff) = value(
            &repository,
            GitRead::WorkingDiff {
                path: "tracked".into(),
                old_path: None,
                staged: false,
            },
        ) else {
            panic!()
        };
        assert_eq!(diff.old_content, "head\n");
        assert!(diff.new_content.is_empty());
        write(
            &repository,
            GitWrite::Discard {
                paths: vec!["tracked".into()],
            },
        );
        fixture.write("tracked", b"binary\0bytes");
        let GitReadValue::Diff(diff) = value(
            &repository,
            GitRead::WorkingDiff {
                path: "tracked".into(),
                old_path: None,
                staged: false,
            },
        ) else {
            panic!()
        };
        assert!(diff.is_binary);
        fixture.write("tracked", &vec![b'x'; cli::MAX_BLOB_BYTES + 1]);
        let GitReadValue::Diff(diff) = value(
            &repository,
            GitRead::WorkingDiff {
                path: "tracked".into(),
                old_path: None,
                staged: false,
            },
        ) else {
            panic!()
        };
        assert!(diff.too_large);
        symlink(&fixture.root, fixture.root.join("alias")).unwrap();
        assert!(
            repository
                .request(GitRead::WorkingDiff {
                    path: "alias/tracked".into(),
                    old_path: None,
                    staged: false
                })
                .unwrap()
                .execute()
                .is_err()
        );
    }

    #[test]
    fn actual_worktree_postconditions_and_discovery_do_not_inject_linked_siblings() {
        let fixture = Fixture::new(&std::env::temp_dir());
        fixture.seed();
        let repository = fixture.repository();
        let target = fixture.root.join("linked");
        let target = target.to_str().unwrap().to_owned();
        let outcome = write(
            &repository,
            GitWrite::WorktreeAdd {
                target: target.clone(),
                branch: GitRef::local("topic").unwrap(),
                create_branch: true,
                base: None,
            },
        );
        assert!(matches!(
            outcome.reconciliation.unwrap().postcondition,
            GitPostcondition::WorktreeCreated { .. }
        ));
        assert_eq!(repository.backend.discover().unwrap().len(), 1);
        let outcome = write(
            &repository,
            GitWrite::WorktreeRemove {
                target: target.clone(),
                force: false,
            },
        );
        assert!(matches!(
            outcome.reconciliation.unwrap().postcondition,
            GitPostcondition::WorktreeRemoved { .. }
        ));
        assert!(!Path::new(&target).exists());
        write(&repository, GitWrite::WorktreePrune);
    }

    #[test]
    fn actual_removing_the_source_worktree_uses_a_verified_survivor_for_reconciliation() {
        let fixture = Fixture::new(&std::env::temp_dir());
        fixture.seed();
        let main = fixture.repository();
        let target = fixture.root.join("linked").to_str().unwrap().to_owned();
        write(
            &main,
            GitWrite::WorktreeAdd {
                target: target.clone(),
                branch: GitRef::local("source-to-remove").unwrap(),
                create_branch: true,
                base: None,
            },
        );
        let mut source = fixture.snapshot.clone();
        source.canonical_path = target.clone();
        let repository = GitBackend::connect(source, GitLifetime::new())
            .unwrap()
            .discover()
            .unwrap()
            .remove(0);
        let outcome = write(
            &repository,
            GitWrite::WorktreeRemove {
                target: target.clone(),
                force: false,
            },
        );
        let reconciliation = outcome.reconciliation.unwrap();
        assert!(matches!(
            reconciliation.postcondition,
            GitPostcondition::WorktreeRemoved { .. }
        ));
        assert_eq!(
            reconciliation.repository.authority.common_dir,
            main.authority.common_dir
        );
        assert!(reconciliation.repository.request(GitRead::Status).is_err());
        assert!(
            reconciliation
                .repository
                .prepare_write(GitWrite::StageAll)
                .is_err()
        );
        assert!(
            reconciliation
                .repository
                .backend()
                .repository_for_file("tracked")
                .is_err()
        );
        assert!(!Path::new(&target).exists());
    }

    #[test]
    #[cfg(target_os = "linux")]
    #[ignore = "requires the isolated GitHub Actions loopback sshd fixture"]
    fn actual_loopback_ssh_git_backend_reads_writes_epoch_and_sftp_containment() {
        let (connection, root) = remote_ssh::loopback_ssh_fixture().unwrap();
        let fixture = Fixture::new(&root);
        fixture.seed();
        let mut source = fixture.snapshot.clone();
        source.backend = ExecutionBackend::Ssh {
            connection_fingerprint: remote_ssh::connection_fingerprint(&connection),
            connection: connection.clone(),
            connection_epoch: None,
        };
        let backend = GitBackend::connect(source.clone(), GitLifetime::new()).unwrap();
        let repository = backend.discover().unwrap().remove(0);
        assert!(matches!(
            backend.snapshot().backend,
            ExecutionBackend::Ssh {
                connection_epoch: Some(_),
                ..
            }
        ));
        let nested = Fixture::new(&fixture.root);
        nested.seed();
        let prefix = nested.root.file_name().unwrap().to_str().unwrap();
        let (nested_repository, relative) = backend
            .repository_for_file(&format!("{prefix}/tracked"))
            .unwrap();
        assert_eq!(
            nested_repository.authority().worktree_root,
            nested.root.to_str().unwrap()
        );
        assert_eq!(relative, "tracked");
        assert!(nested_repository.matches_snapshot(backend.snapshot()));
        nested.write("literal\\name\n", b"remote literal bytes\n");
        let (nested_repository, relative) = backend
            .repository_for_file(&format!("{prefix}/literal\\name\n"))
            .unwrap();
        assert_eq!(relative, "literal\\name\n");
        let GitReadValue::Diff(diff) = value(
            &nested_repository,
            GitRead::WorkingDiff {
                path: relative,
                old_path: None,
                staged: false,
            },
        ) else {
            panic!()
        };
        assert!(diff.old_content.is_empty());
        assert_eq!(diff.new_content, "remote literal bytes\n");
        fixture.write("tracked", b"remote working\n");
        let GitReadValue::Diff(diff) = value(
            &repository,
            GitRead::WorkingDiff {
                path: "tracked".into(),
                old_path: None,
                staged: false,
            },
        ) else {
            panic!()
        };
        assert_eq!(diff.new_content, "remote working\n");
        let remote = fixture.root.join(".git/fixture-remote.git");
        fixture.git(&["init", "--bare", remote.to_str().unwrap()]);
        fixture.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        fixture.git(&["config", "push.autoSetupRemote", "true"]);
        let hostile = ":(glob)* line\nremote";
        fixture.write(hostile, b"exact remote bytes\n");
        write(
            &repository,
            GitWrite::Stage {
                paths: vec![hostile.into()],
            },
        );
        write(
            &repository,
            GitWrite::Unstage {
                paths: vec![hostile.into()],
            },
        );
        write(
            &repository,
            GitWrite::Discard {
                paths: vec![hostile.into()],
            },
        );
        assert!(!fixture.root.join(hostile).exists());
        symlink(&fixture.root, fixture.root.join("alias")).unwrap();
        assert!(
            repository
                .request(GitRead::WorkingDiff {
                    path: "alias/tracked".into(),
                    old_path: None,
                    staged: false
                })
                .unwrap()
                .execute()
                .is_err()
        );
        write(
            &repository,
            GitWrite::Stage {
                paths: vec!["tracked".into()],
            },
        );
        write(
            &repository,
            GitWrite::Commit {
                message: "remote host commit".into(),
            },
        );
        write(&repository, GitWrite::Push);
        write(&repository, GitWrite::Pull);
        let target = fixture.root.join("ssh-linked").to_str().unwrap().to_owned();
        write(
            &repository,
            GitWrite::WorktreeAdd {
                target: target.clone(),
                branch: GitRef::local("ssh-topic").unwrap(),
                create_branch: true,
                base: None,
            },
        );
        write(
            &repository,
            GitWrite::WorktreeRemove {
                target,
                force: false,
            },
        );
        write(&repository, GitWrite::WorktreePrune);
        let prepared = repository
            .prepare_write(GitWrite::Commit {
                message: "must not run".into(),
            })
            .unwrap();
        remote_ssh::invalidate_connection(&connection.id);
        let outcome = prepared.execute();
        assert_eq!(outcome.state, GitWriteState::NotDispatched);
        assert!(repository.busy().is_none());
        let fresh = GitBackend::connect(source, GitLifetime::new()).unwrap();
        assert_ne!(
            fresh.snapshot().source_signature(),
            backend.snapshot().source_signature()
        );
        assert_eq!(
            GitBackend::connect(backend.snapshot().clone(), GitLifetime::new())
                .err()
                .unwrap()
                .kind,
            GitErrorKind::Stale
        );
        let mut stale_file_anchor = backend.snapshot().clone();
        stale_file_anchor.canonical_path =
            fixture.root.join("tracked").to_str().unwrap().to_owned();
        // Epoch rejection must precede the non-directory anchor's SFTP error.
        assert_eq!(
            GitBackend::connect(stale_file_anchor, GitLifetime::new())
                .err()
                .unwrap()
                .kind,
            GitErrorKind::Stale
        );
        let same_epoch = GitBackend::connect(fresh.snapshot().clone(), GitLifetime::new()).unwrap();
        assert_eq!(
            same_epoch.snapshot().source_signature(),
            fresh.snapshot().source_signature()
        );
        remote_ssh::invalidate_connection(&connection.id);
    }

    #[test]
    #[cfg(target_os = "linux")]
    #[ignore = "requires the isolated GitHub Actions loopback sshd fixture"]
    fn actual_loopback_ssh_dispatched_timeout_keeps_ownership_until_explicit_review() {
        use std::os::unix::fs::PermissionsExt;
        let (connection, root) = remote_ssh::loopback_ssh_fixture().unwrap();
        let fixture = Fixture::new(&root);
        fixture.seed();
        fixture.write("tracked", b"pending commit\n");
        fixture.git(&["add", "--", "tracked"]);
        let hook = fixture.root.join(".git/hooks/pre-commit");
        std::fs::write(
            &hook,
            b"#!/bin/sh\nprintf '%s' \"$PPID\" > hook-git-pid\nsleep 3\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut source = fixture.snapshot.clone();
        source.backend = ExecutionBackend::Ssh {
            connection_fingerprint: remote_ssh::connection_fingerprint(&connection),
            connection: connection.clone(),
            connection_epoch: None,
        };
        let backend = GitBackend::connect(source, GitLifetime::new()).unwrap();
        let repository = backend.discover().unwrap().remove(0);
        let mut prepared = repository
            .prepare_write(GitWrite::Commit {
                message: "fixture timeout".into(),
            })
            .unwrap();
        let id = prepared.id();
        prepared.fixture_timeout(Duration::from_secs(1));
        let outcome = prepared.execute();
        assert_eq!(outcome.state, GitWriteState::Uncertain);
        assert!(outcome.lease_retained);
        assert_eq!(repository.busy().unwrap().operation_id, id);
        // The PID was written by this private hook, not taken from user state.
        // Absence is required before making the review acknowledgement.
        let pid = std::fs::read_to_string(fixture.root.join("hook-git-pid"))
            .unwrap()
            .parse::<u32>()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while Path::new("/proc").join(pid.to_string()).exists() {
            assert!(
                Instant::now() < deadline,
                "fixture Git process did not stop"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        repository
            .review_uncertain(id, UncertainReview::UserConfirmedOriginalOperationStopped)
            .unwrap();
        assert!(repository.busy().is_none());
        remote_ssh::invalidate_connection(&connection.id);
    }
}
