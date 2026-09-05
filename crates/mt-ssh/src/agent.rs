//! Bounded, exact-route remote agent process inventory.
//!
//! The probe runs on an already authenticated pooled SSH session. It inspects
//! Linux `/proc` remotely but returns only a normalized provider plus PID/start
//! ticks. Full environment and command-line data never cross the SSH channel.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use mt_identity::{
    ExecutionHostId, PaneKey, TabId, TerminalIncarnationId, TerminalSessionId, WorktreeId,
};

use crate::pool::{BoundedExecOutput, BoundedExecState, CachedSession};
use crate::run_bounded_exec_on_session;

const AGENT_OUTPUT_CAP_BYTES: usize = 16 * 1024;
const AGENT_PROCESS_CAP: usize = 64;
const INVENTORY_HEADER: &str = "mini-term-agent-inventory-v1";
const INVENTORY_END: &str = "end";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteAgentRoute {
    pub protocol_version: u32,
    pub execution_host_id: ExecutionHostId,
    pub worktree_id: WorktreeId,
    pub tab_id: TabId,
    pub pane_key: PaneKey,
    pub terminal_session_id: TerminalSessionId,
    pub terminal_incarnation_id: TerminalIncarnationId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RemoteAgentProvider {
    Claude,
    Codex,
    OpenCode,
    Pi,
    Grok,
}

impl RemoteAgentProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Pi => "pi",
            Self::Grok => "grok",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::OpenCode),
            "pi" => Some(Self::Pi),
            "grok" => Some(Self::Grok),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteAgentCapability {
    LinuxProc,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RemoteAgentProcess {
    pub provider: RemoteAgentProvider,
    pub pid: u32,
    pub start_ticks: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteAgentInventory {
    pub connection_epoch: u64,
    pub capability: RemoteAgentCapability,
    pub processes: Vec<RemoteAgentProcess>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RemoteAgentProbeErrorKind {
    Transport,
    State,
    Protocol,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteAgentProbeError {
    kind: RemoteAgentProbeErrorKind,
    message: String,
    retryable: bool,
    retire_session: bool,
}

impl RemoteAgentProbeError {
    fn transport(message: impl Into<String>, retire_session: bool) -> Self {
        Self {
            kind: RemoteAgentProbeErrorKind::Transport,
            message: message.into(),
            retryable: true,
            retire_session,
        }
    }

    fn state(message: impl Into<String>) -> Self {
        Self {
            kind: RemoteAgentProbeErrorKind::State,
            message: message.into(),
            retryable: false,
            retire_session: false,
        }
    }

    fn protocol(message: impl Into<String>) -> Self {
        Self {
            kind: RemoteAgentProbeErrorKind::Protocol,
            message: message.into(),
            retryable: false,
            retire_session: false,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub const fn should_retry(&self) -> bool {
        self.retryable
    }

    pub const fn requires_session_retirement(&self) -> bool {
        self.retire_session
    }

    pub const fn is_transport(&self) -> bool {
        matches!(self.kind, RemoteAgentProbeErrorKind::Transport)
    }
}

impl fmt::Display for RemoteAgentProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RemoteAgentProbeError {}

pub async fn inspect_remote_agents(
    session: Arc<CachedSession>,
    route: &RemoteAgentRoute,
    timeout: Duration,
) -> Result<RemoteAgentInventory, RemoteAgentProbeError> {
    let command = build_probe_command(route);
    let output =
        run_bounded_exec_on_session(session.as_ref(), &command, timeout, AGENT_OUTPUT_CAP_BYTES)
            .await
            .map_err(|error| RemoteAgentProbeError::transport(error, true))?;
    let output = classify_exec_output(output)?;
    let capability = parse_inventory(&output.stdout)?;
    Ok(RemoteAgentInventory {
        connection_epoch: session.connection_epoch().get(),
        capability: capability.0,
        processes: capability.1,
    })
}

fn classify_exec_output(
    output: BoundedExecOutput,
) -> Result<BoundedExecOutput, RemoteAgentProbeError> {
    if output.requires_session_retirement() {
        return Err(RemoteAgentProbeError::transport(
            "remote agent probe left the SSH channel state uncertain",
            true,
        ));
    }
    if output.timed_out {
        return Err(RemoteAgentProbeError::transport(
            "remote agent probe timed out",
            false,
        ));
    }
    if output.stdout_truncated || output.stderr_truncated {
        return Err(RemoteAgentProbeError::protocol(
            "remote agent probe exceeded its output limit",
        ));
    }
    if output.state != BoundedExecState::Started {
        return Err(RemoteAgentProbeError::state(
            "remote server rejected the agent probe",
        ));
    }
    match output.exit_code {
        Some(0) => Ok(output),
        Some(_) => Err(RemoteAgentProbeError::state("remote agent probe failed")),
        None => Err(RemoteAgentProbeError::protocol(
            "remote agent probe returned no exit status",
        )),
    }
}

fn parse_inventory(
    bytes: &[u8],
) -> Result<(RemoteAgentCapability, Vec<RemoteAgentProcess>), RemoteAgentProbeError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RemoteAgentProbeError::protocol("remote agent inventory is not UTF-8"))?;
    if !text.ends_with('\n') || text.contains('\0') || text.contains('\r') {
        return Err(RemoteAgentProbeError::protocol(
            "remote agent inventory framing is malformed",
        ));
    }
    let mut lines = text.lines();
    if lines.next() != Some(INVENTORY_HEADER) {
        return Err(RemoteAgentProbeError::protocol(
            "remote agent inventory header is invalid",
        ));
    }
    let capability = match lines.next() {
        Some("capability=linux-proc") => RemoteAgentCapability::LinuxProc,
        Some("capability=unsupported") => RemoteAgentCapability::Unsupported,
        _ => {
            return Err(RemoteAgentProbeError::protocol(
                "remote agent inventory capability is invalid",
            ));
        }
    };

    let mut processes = Vec::new();
    let mut seen = HashSet::new();
    let mut ended = false;
    for line in lines {
        if ended {
            return Err(RemoteAgentProbeError::protocol(
                "remote agent inventory has data after its footer",
            ));
        }
        if line == INVENTORY_END {
            ended = true;
            continue;
        }
        if line == "truncated=1" {
            return Err(RemoteAgentProbeError::protocol(
                "remote agent inventory exceeded its process limit",
            ));
        }
        if capability == RemoteAgentCapability::Unsupported {
            return Err(RemoteAgentProbeError::protocol(
                "unsupported remote agent inventory contains process rows",
            ));
        }
        let mut fields = line.split('\t');
        if fields.next() != Some("agent") {
            return Err(RemoteAgentProbeError::protocol(
                "remote agent inventory row has an invalid kind",
            ));
        }
        let provider = fields
            .next()
            .and_then(RemoteAgentProvider::parse)
            .ok_or_else(|| {
                RemoteAgentProbeError::protocol("remote agent inventory provider is invalid")
            })?;
        let pid = parse_positive::<u32>(fields.next(), "PID")?;
        let start_ticks = parse_positive::<u64>(fields.next(), "start ticks")?;
        if fields.next().is_some() {
            return Err(RemoteAgentProbeError::protocol(
                "remote agent inventory row has extra fields",
            ));
        }
        if processes.len() >= AGENT_PROCESS_CAP {
            return Err(RemoteAgentProbeError::protocol(
                "remote agent inventory exceeded its process limit",
            ));
        }
        let identity = (pid, start_ticks);
        if !seen.insert(identity) {
            return Err(RemoteAgentProbeError::protocol(
                "remote agent inventory contains a duplicate process",
            ));
        }
        processes.push(RemoteAgentProcess {
            provider,
            pid,
            start_ticks,
        });
    }
    if !ended {
        return Err(RemoteAgentProbeError::protocol(
            "remote agent inventory footer is missing",
        ));
    }
    processes.sort_by_key(|process| (process.start_ticks, process.pid));
    Ok((capability, processes))
}

fn parse_positive<T>(value: Option<&str>, label: &str) -> Result<T, RemoteAgentProbeError>
where
    T: std::str::FromStr + Default + PartialEq,
{
    let value = value.ok_or_else(|| {
        RemoteAgentProbeError::protocol(format!("remote agent inventory {label} is missing"))
    })?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(RemoteAgentProbeError::protocol(format!(
            "remote agent inventory {label} is invalid"
        )));
    }
    let parsed = value.parse::<T>().map_err(|_| {
        RemoteAgentProbeError::protocol(format!("remote agent inventory {label} is invalid"))
    })?;
    if parsed == T::default() {
        return Err(RemoteAgentProbeError::protocol(format!(
            "remote agent inventory {label} must be nonzero"
        )));
    }
    Ok(parsed)
}

fn build_probe_command(route: &RemoteAgentRoute) -> String {
    let expected = [
        (
            "MINITERM_AGENT_PROTOCOL_VERSION",
            route.protocol_version.to_string(),
        ),
        (
            "MINITERM_EXECUTION_HOST_ID",
            route.execution_host_id.to_string(),
        ),
        ("MINITERM_WORKTREE_ID", route.worktree_id.to_string()),
        ("MINITERM_TAB_ID", route.tab_id.to_string()),
        ("MINITERM_PANE_KEY", route.pane_key.to_string()),
        (
            "MINITERM_TERMINAL_SESSION_ID",
            route.terminal_session_id.to_string(),
        ),
        (
            "MINITERM_TERMINAL_INCARNATION_ID",
            route.terminal_incarnation_id.to_string(),
        ),
    ];
    let checks = expected
        .iter()
        .map(|(key, value)| {
            format!(
                "has_line {} || continue",
                shell_quote(&format!("{key}={value}"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"set -f
printf '{INVENTORY_HEADER}\n'
if [ ! -d /proc ] || ! command -v tr >/dev/null 2>&1 || ! command -v head >/dev/null 2>&1 || ! command -v readlink >/dev/null 2>&1; then
  printf 'capability=unsupported\n{INVENTORY_END}\n'
  exit 0
fi
printf 'capability=linux-proc\n'
count=0
# Expand only the fixed process list; argv and stat splitting stay literal.
set +f
for proc in /proc/[0-9]*; do
  set -f
  [ -r "$proc/environ" ] || continue
  env_lines=$(tr '\000' '\n' < "$proc/environ" 2>/dev/null) || continue
  has_line() {{
    case "
$env_lines
" in
      *"
$1
"*) return 0 ;;
      *) return 1 ;;
    esac
  }}
{checks}
  provider=''
  executable=$(readlink "$proc/exe" 2>/dev/null || :)
  executable=${{executable##*/}}
  case "$executable" in
    claude|claude-code) provider='claude' ;;
    codex|codex.exe) provider='codex' ;;
    opencode|opencode.exe) provider='opencode' ;;
    pi|pi.exe) provider='pi' ;;
    grok|grok.exe) provider='grok' ;;
  esac
  if [ -z "$provider" ] && [ -r "$proc/cmdline" ]; then
    args=$(tr '\000' '\n' < "$proc/cmdline" 2>/dev/null | head -n 4)
    old_ifs=$IFS
    IFS='
'
    for arg in $args; do
      case "$arg" in
        claude|claude-code|*/claude|*/claude-code|*/@anthropic-ai/claude-code/*) provider='claude' ;;
        codex|*/codex|*/@openai/codex/*) provider='codex' ;;
        opencode|*/opencode|*/opencode-ai/*) provider='opencode' ;;
        pi|*/pi|*/pi-agent/*) provider='pi' ;;
        grok|*/grok|*/grok-cli/*) provider='grok' ;;
      esac
      [ -z "$provider" ] || break
    done
    IFS=$old_ifs
  fi
  [ -n "$provider" ] || continue
  IFS= read -r stat_line < "$proc/stat" 2>/dev/null || continue
  stat_tail=${{stat_line##*) }}
  [ "$stat_tail" != "$stat_line" ] || continue
  set -- $stat_tail
  [ "$#" -ge 20 ] || continue
  shift 19
  start_ticks=$1
  case "$start_ticks" in ''|*[!0-9]*) continue ;; esac
  pid=${{proc##*/}}
  case "$pid" in ''|*[!0-9]*) continue ;; esac
  if [ "$count" -ge {AGENT_PROCESS_CAP} ]; then
    printf 'truncated=1\n'
    break
  fi
  printf 'agent\t%s\t%s\t%s\n' "$provider" "$pid" "$start_ticks"
  count=$((count + 1))
done
printf '{INVENTORY_END}\n'"#
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_identity::{HostInstallId, RepoId};

    fn route() -> RemoteAgentRoute {
        let host = ExecutionHostId::derive("SHA256:server", &HostInstallId::new());
        let repo = RepoId::derive(&host, "/repo/.git");
        RemoteAgentRoute {
            protocol_version: 1,
            execution_host_id: host,
            worktree_id: WorktreeId::derive(&repo, "/repo", None),
            tab_id: TabId::new(),
            pane_key: PaneKey::new(),
            terminal_session_id: TerminalSessionId::new(),
            terminal_incarnation_id: TerminalIncarnationId::new(),
        }
    }

    #[test]
    fn parser_accepts_supported_and_unsupported_inventory() {
        let supported = parse_inventory(
            b"mini-term-agent-inventory-v1\ncapability=linux-proc\nagent\tclaude\t42\t900\nagent\tcodex\t7\t100\nend\n",
        )
        .unwrap();
        assert_eq!(supported.0, RemoteAgentCapability::LinuxProc);
        assert_eq!(supported.1.len(), 2);
        assert_eq!(supported.1[0].provider, RemoteAgentProvider::Codex);
        assert_eq!(supported.1[1].pid, 42);

        assert_eq!(
            parse_inventory(b"mini-term-agent-inventory-v1\ncapability=unsupported\nend\n")
                .unwrap(),
            (RemoteAgentCapability::Unsupported, Vec::new())
        );
    }

    #[test]
    fn parser_rejects_ambiguous_or_unbounded_rows() {
        for invalid in [
            b"capability=linux-proc\nend\n".as_slice(),
            b"mini-term-agent-inventory-v1\ncapability=linux-proc\nagent\tunknown\t1\t2\nend\n",
            b"mini-term-agent-inventory-v1\ncapability=linux-proc\nagent\tclaude\t0\t2\nend\n",
            b"mini-term-agent-inventory-v1\ncapability=linux-proc\nagent\tclaude\t1\t2\nagent\tcodex\t1\t2\nend\n",
            b"mini-term-agent-inventory-v1\ncapability=linux-proc\ntruncated=1\nend\n",
            b"mini-term-agent-inventory-v1\ncapability=unsupported\nagent\tclaude\t1\t2\nend\n",
            b"mini-term-agent-inventory-v1\ncapability=linux-proc\n",
            b"mini-term-agent-inventory-v1\ncapability=linux-proc\nend\nextra\n",
        ] {
            assert!(parse_inventory(invalid).is_err(), "accepted {invalid:?}");
        }
        assert!(parse_inventory(&[0xff]).is_err());
    }

    #[test]
    fn command_matches_every_route_field_without_emitting_raw_process_data() {
        let route = route();
        let command = build_probe_command(&route);
        for expected in [
            route.execution_host_id.as_str(),
            route.worktree_id.as_str(),
            route.tab_id.as_str(),
            route.pane_key.as_str(),
            route.terminal_session_id.as_str(),
            route.terminal_incarnation_id.as_str(),
        ] {
            assert!(command.contains(expected));
        }
        assert!(command.contains("has_line 'MINITERM_AGENT_PROTOCOL_VERSION=1'"));
        assert!(command.contains("printf 'agent\\t%s\\t%s\\t%s\\n'"));
        assert!(!command.contains("printf '%s' \"$env_lines\""));
        assert!(!command.contains("printf '%s' \"$args\""));
    }

    #[cfg(target_os = "linux")]
    mod generated_probe {
        use super::*;
        use std::io::{BufRead, BufReader, Read, Write};
        use std::os::unix::process::CommandExt;
        use std::path::{Path, PathBuf};
        use std::process::{Child, Command, ExitStatus, Stdio};
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        const TIMEOUT: Duration = Duration::from_secs(10);
        const READY: &str = "mini-term-probe-fixture-ready";

        struct OwnedChild(Child);

        impl OwnedChild {
            fn wait(&mut self) -> ExitStatus {
                let deadline = Instant::now() + TIMEOUT;
                loop {
                    if let Some(status) = self.0.try_wait().unwrap() {
                        return status;
                    }
                    assert!(Instant::now() < deadline, "probe child did not exit");
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }

        impl Drop for OwnedChild {
            fn drop(&mut self) {
                // Only children created by this test are ever terminated.
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        struct FixtureDirectory(PathBuf);

        impl FixtureDirectory {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "mini-term-agent-probe-{}",
                    TerminalSessionId::new()
                ));
                std::fs::create_dir(&path).unwrap();
                std::fs::write(path.join("codex"), b"").unwrap();
                Self(path)
            }
        }

        impl Drop for FixtureDirectory {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        #[ignore = "subprocess fixture for generated_probe tests"]
        fn linux_probe_fixture() {
            if std::env::var_os("MINITERM_PROBE_TEST_FIXTURE").is_none() {
                return;
            }
            println!("{READY}");
            std::io::stdout().flush().unwrap();
            let mut shutdown = [0];
            let _ = std::io::stdin().read(&mut shutdown).unwrap();
        }

        fn fixture(route: &RemoteAgentRoute, argv0: &str, cwd: &Path) -> OwnedChild {
            let mut child = OwnedChild(
                Command::new(std::env::current_exe().unwrap())
                    .arg0(argv0)
                    .args([
                        "--exact",
                        "agent::tests::generated_probe::linux_probe_fixture",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env_clear()
                    .env("MINITERM_PROBE_TEST_FIXTURE", "1")
                    .env(
                        "MINITERM_AGENT_PROTOCOL_VERSION",
                        route.protocol_version.to_string(),
                    )
                    .env(
                        "MINITERM_EXECUTION_HOST_ID",
                        route.execution_host_id.as_str(),
                    )
                    .env("MINITERM_WORKTREE_ID", route.worktree_id.as_str())
                    .env("MINITERM_TAB_ID", route.tab_id.as_str())
                    .env("MINITERM_PANE_KEY", route.pane_key.as_str())
                    .env(
                        "MINITERM_TERMINAL_SESSION_ID",
                        route.terminal_session_id.as_str(),
                    )
                    .env(
                        "MINITERM_TERMINAL_INCARNATION_ID",
                        route.terminal_incarnation_id.as_str(),
                    )
                    .current_dir(cwd)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let stdout = child.0.stdout.take().unwrap();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut ready = false;
                for line in BufReader::new(stdout.take(4096)).lines() {
                    if !ready && line.is_ok_and(|line| line.ends_with(READY)) {
                        ready = true;
                        let _ = tx.send(true);
                    }
                }
                if !ready {
                    let _ = tx.send(false);
                }
            });
            assert!(
                rx.recv_timeout(TIMEOUT)
                    .expect("fixture readiness timed out")
            );
            child
        }

        fn probe(route: &RemoteAgentRoute, cwd: &Path) -> Vec<RemoteAgentProcess> {
            let mut child = OwnedChild(
                Command::new("/bin/sh")
                    .args(["-f", "-c", &build_probe_command(route)])
                    .env_clear()
                    .env("PATH", "/usr/bin:/bin")
                    .current_dir(cwd)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let stdout = child.0.stdout.take().unwrap();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let result = stdout.take(16 * 1024 + 1).read_to_end(&mut bytes);
                let _ = tx.send(result.map(|_| bytes));
            });
            let bytes = rx.recv_timeout(TIMEOUT).expect("probe timed out").unwrap();
            assert!(child.wait().success());
            let (capability, processes) = parse_inventory(&bytes).unwrap();
            assert_eq!(capability, RemoteAgentCapability::LinuxProc);
            processes
        }

        #[test]
        fn generated_command_discovers_only_exact_route_and_process_identity() {
            let route = route();
            let cwd = FixtureDirectory::new();
            let mut fixture = fixture(&route, "codex", &cwd.0);
            let found = probe(&route, &cwd.0);
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].pid, fixture.0.id());
            assert_eq!(found[0].provider, RemoteAgentProvider::Codex);
            assert!(found[0].start_ticks > 0);
            assert_eq!(probe(&route, &cwd.0), found);

            for field in 0..7 {
                let mut mismatch = route.clone();
                match field {
                    0 => mismatch.protocol_version += 1,
                    1 => {
                        mismatch.execution_host_id =
                            ExecutionHostId::derive("other", &HostInstallId::new());
                    }
                    2 => {
                        let repo = RepoId::derive(&route.execution_host_id, "/other/.git");
                        mismatch.worktree_id = WorktreeId::derive(&repo, "/other", None);
                    }
                    3 => mismatch.tab_id = TabId::new(),
                    4 => mismatch.pane_key = PaneKey::new(),
                    5 => mismatch.terminal_session_id = TerminalSessionId::new(),
                    6 => mismatch.terminal_incarnation_id = TerminalIncarnationId::new(),
                    _ => unreachable!(),
                }
                assert!(probe(&mismatch, &cwd.0).is_empty(), "route field {field}");
            }
            drop(fixture.0.stdin.take());
            assert!(fixture.wait().success());
            assert!(probe(&route, &cwd.0).is_empty());
        }

        #[test]
        fn generated_command_keeps_wildcard_argv_literal() {
            let route = route();
            let cwd = FixtureDirectory::new();
            let _fixture = fixture(&route, "*", &cwd.0);
            assert!(probe(&route, &cwd.0).is_empty());
        }

        #[test]
        fn generated_command_normalizes_supported_provider_arguments() {
            let route = route();
            let cwd = FixtureDirectory::new();
            for (argv0, provider) in [
                ("claude-code", RemoteAgentProvider::Claude),
                ("codex", RemoteAgentProvider::Codex),
                ("opencode", RemoteAgentProvider::OpenCode),
                ("pi", RemoteAgentProvider::Pi),
                ("grok", RemoteAgentProvider::Grok),
            ] {
                let fixture = fixture(&route, argv0, &cwd.0);
                let found = probe(&route, &cwd.0);
                assert_eq!(found.len(), 1);
                assert_eq!(found[0].pid, fixture.0.id());
                assert_eq!(found[0].provider, provider);
            }
        }
    }

    #[test]
    fn uncertain_and_truncated_exec_results_fail_closed() {
        let uncertain = BoundedExecOutput {
            state: BoundedExecState::ChannelOpenUnknown,
            timed_out: true,
            channel_cleanup_uncertain: true,
            ..BoundedExecOutput::default()
        };
        let error = classify_exec_output(uncertain).unwrap_err();
        assert!(error.should_retry());
        assert!(error.requires_session_retirement());

        let truncated = BoundedExecOutput {
            state: BoundedExecState::Started,
            stdout_truncated: true,
            exit_code: Some(0),
            ..BoundedExecOutput::default()
        };
        let error = classify_exec_output(truncated).unwrap_err();
        assert!(!error.should_retry());
    }
}
