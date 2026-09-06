//! Bounded, exact-route remote agent process inventory.
//!
//! The probe runs on an already authenticated pooled SSH session. It inspects
//! Linux `/proc` remotely but returns only a normalized provider plus PID/start
//! ticks and foreground ownership. Full environment and command-line data never
//! cross the SSH channel.

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
const INVENTORY_HEADER: &str = "mini-term-agent-inventory-v2";
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
    /// Logical CLI process group owns the managed terminal's foreground TTY.
    /// This is identity/liveness evidence, never a task activity classification.
    pub foreground: bool,
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
    if bytes.len() > AGENT_OUTPUT_CAP_BYTES {
        return Err(RemoteAgentProbeError::protocol(
            "remote agent inventory exceeded its output limit",
        ));
    }
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
        let foreground = match fields.next() {
            Some("foreground") => true,
            Some("background") => false,
            _ => {
                return Err(RemoteAgentProbeError::protocol(
                    "remote agent inventory foreground ownership is invalid",
                ));
            }
        };
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
            foreground,
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
                "has_line {} || return 1",
                shell_quote(&format!("{key}={value}"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"set -f
exec 2>/dev/null
LC_ALL=C
export LC_ALL
printf '{INVENTORY_HEADER}\n'
unsupported() {{
  printf 'capability=unsupported\n{INVENTORY_END}\n'
  exit 0
}}
malformed() {{
  printf 'capability=linux-proc\ninvalid=1\n{INVENTORY_END}\n'
  exit 0
}}
for tool in tr head readlink awk; do
  command -v "$tool" >/dev/null 2>&1 || unsupported
done
[ -d /proc ] || unsupported
positive() {{
  case "$1" in ''|0|*[!0-9]*) return 1 ;; esac
}}
read_stat() {{
  stat_line=$(head -c 4097 "$1/stat") || return 1
  [ "${{#stat_line}}" -le 4096 ] || return 1
  stat_tail=${{stat_line##*) }}
  [ "$stat_tail" != "$stat_line" ] || return 1
  set -- $stat_tail
  [ "$#" -ge 20 ] || return 1
  s_state=$1 s_ppid=$2 s_pgid=$3 s_sid=$4 s_tty=$5 s_tpgid=$6
  shift 19
  s_start=$1
  positive "$s_start" && positive "$s_pgid" && positive "$s_sid" || return 1
  case "$s_ppid" in ''|*[!0-9]*) return 1 ;; esac
  case "$s_tty" in ''|*[!0-9]*) return 1 ;; esac
  case "$s_tpgid" in -1|0) : ;; *) positive "$s_tpgid" || return 1 ;; esac
}}
newline='
'
read_nul_file() {{
  # Carry head's status through tr and preserve the final NUL as a newline.
  nul_lines=$({{ head -c "$2" "$1"; printf '\000%s' "$?"; }} | tr '\000\n' '\n?') || return 1
  case "$nul_lines" in
    *"$newline"0) nul_lines=${{nul_lines%"$newline"0}} ;;
    *) return 1 ;;
  esac
  [ "${{#nul_lines}}" -lt "$2" ] || return 1
  case "$nul_lines" in *"$newline") : ;; *) return 1 ;; esac
}}
read_env() {{
  [ -r "$1/environ" ] || return 1
  read_nul_file "$1/environ" 65537 || return 1
  env_lines=$nul_lines
}}
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
matches_route() {{
{checks}
}}
root_fields() {{
  r_pid='' r_start='' r_tty=''
  old_ifs=$IFS
  IFS='
'
  for line in $env_lines; do
    case "$line" in
      MINITERM_MANAGED_ROOT_PID=*) r_pid=${{line#*=}} ;;
      MINITERM_MANAGED_ROOT_START_TICKS=*) r_start=${{line#*=}} ;;
      MINITERM_MANAGED_ROOT_TTY=*) r_tty=${{line#*=}} ;;
    esac
  done
  IFS=$old_ifs
  positive "$r_pid" && positive "$r_start" && positive "$r_tty"
}}
# Only this fixed enumeration is glob-expanded; all remote text stays literal.
set +f
set -- /proc/[0-9]*
set -f
[ "$#" -le 8192 ] || unsupported
process_paths=$*
root_pid=''
for proc in $process_paths; do
  read_env "$proc" || continue
  matches_route || continue
  root_fields || continue
  [ "${{proc##*/}}" = "$r_pid" ] || continue
  read_stat "$proc" || malformed
  [ "$s_start" = "$r_start" ] && [ "$s_tty" = "$r_tty" ] || continue
  case "$s_state" in Z|X) continue ;; esac
  [ -z "$root_pid" ] || unsupported
  root_pid=$r_pid root_start=$r_start root_tty=$r_tty root_sid=$s_sid root_foreground=$s_tpgid
done
[ -n "$root_pid" ] || unsupported
positive "$root_foreground" || unsupported

owned_ancestry() {{
  ancestor=$1 previous_start=$2 depth=0
  while [ "$ancestor" != "$root_pid" ]; do
    positive "$ancestor" || return 1
    [ "$depth" -lt 64 ] || unsupported
    read_stat "/proc/$ancestor" || unsupported
    [ "$s_sid" = "$root_sid" ] && [ "$s_tty" = "$root_tty" ] || return 1
    [ "$s_start" -le "$previous_start" ] || malformed
    [ "$s_ppid" != "$ancestor" ] || malformed
    previous_start=$s_start ancestor=$s_ppid
    depth=$((depth + 1))
  done
  [ "$previous_start" -ge "$root_start" ]
}}

next_arg() {{
  [ -n "$remaining_args" ] || return 1
  arg=${{remaining_args%%"$newline"*}}
  remaining_args=${{remaining_args#*"$newline"}}
}}
classify() {{
  provider='' role=cli
  executable_path=$(readlink "$1/exe") || unsupported
  read_nul_file "$1/cmdline" 4097 || unsupported
  args=$nul_lines
  executable_check=$(readlink "$1/exe") || unsupported
  read_nul_file "$1/cmdline" 4097 || unsupported
  [ "$executable_path" = "$executable_check" ] && [ "$args" = "$nul_lines" ] || unsupported
  executable=$executable_path
  executable=${{executable%" (deleted)"}}
  executable=${{executable##*/}}
  remaining_args=$args
  next_arg || return 1
  case "$executable" in
    claude|claude-code) provider=claude ;;
    codex|codex.exe) provider=codex ;;
    opencode|opencode.exe) provider=opencode ;;
    pi|pi.exe) provider=pi ;;
    grok|grok.exe) provider=grok ;;
    node|nodejs|bun)
      # Only the interpreter entrypoint identifies a CLI, never prompt/options text.
      next_arg || return 1
      [ "$arg" != -- ] || next_arg || return 1
      case "$arg" in
        */@openai/codex/bin/codex.js) provider=codex role=launcher ;;
        */opencode-ai/bin/opencode) provider=opencode role=launcher ;;
        */@anthropic-ai/claude-code/cli.js) provider=claude ;;
        */@earendil-works/pi-coding-agent/dist/cli.js|*/@mariozechner/pi-coding-agent/dist/cli.js) provider=pi ;;
        */grok-cli/dist/index.js) provider=grok ;;
      esac
      ;;
  esac
  [ -n "$provider" ] || return 1
  while next_arg; do
    case "$arg" in
      -h|--help|-v|--version|--print) return 1 ;;
      -p) [ "$provider" = codex ] || return 1; next_arg || return 1 ;;
      -C|--cd|--cwd|-c|--config|-m|--model|--profile|--session|--session-id|-s|--sandbox|-a|--ask-for-approval|-i|--image|--add-dir)
        next_arg || return 1
        ;;
      --) return 0 ;;
      -*) : ;;
      exec|e|app-server|mcp-server|mcp|login|logout|completion|completions|update|upgrade) return 1 ;;
      *) return 0 ;;
    esac
  done
}}

rows='' count=0
for proc in $process_paths; do
  set -f
  if ! read_stat "$proc"; then
    if [ -d "$proc" ] && read_env "$proc" && matches_route; then malformed; fi
    continue
  fi
  [ "$s_sid" = "$root_sid" ] && [ "$s_tty" = "$root_tty" ] || continue
  case "$s_state" in Z|X) continue ;; esac
  pid=${{proc##*/}} start_ticks=$s_start parent=$s_ppid group=$s_pgid
  owned_ancestry "$pid" "$start_ticks" || continue
  read_env "$proc" || unsupported
  matches_route || continue
  root_fields || unsupported
  [ "$r_pid" = "$root_pid" ] && [ "$r_start" = "$root_start" ] && [ "$r_tty" = "$root_tty" ] || continue
  classify "$proc" || continue
  # A process may exit or exec while inspected. Do not turn a raced capture into absence.
  read_stat "$proc" || unsupported
  [ "$s_start" = "$start_ticks" ] && [ "$s_ppid" = "$parent" ] && [ "$s_pgid" = "$group" ] || unsupported
  [ "$s_sid" = "$root_sid" ] && [ "$s_tty" = "$root_tty" ] && [ "$s_tpgid" = "$root_foreground" ] || unsupported
  case "$s_state" in Z|X) unsupported ;; esac
  foreground=background
  [ "$group" != "$root_foreground" ] || foreground=foreground
  rows="$rows$provider $pid $start_ticks $foreground $parent $group $role
"
  count=$((count + 1))
  [ "$count" -le 128 ] || unsupported
done
# Recheck all buffered process identities before publishing the final frame.
old_ifs=$IFS
IFS='
'
for row in $rows; do
  IFS=$old_ifs
  set -- $row
  row_pid=$2 row_start=$3 row_parent=$5 row_group=$6
  read_stat "/proc/$row_pid" || unsupported
  [ "$s_start" = "$row_start" ] && [ "$s_ppid" = "$row_parent" ] && [ "$s_pgid" = "$row_group" ] || unsupported
  [ "$s_tty" = "$root_tty" ] && [ "$s_sid" = "$root_sid" ] && [ "$s_tpgid" = "$root_foreground" ] || unsupported
  case "$s_state" in Z|X) unsupported ;; esac
done
IFS=$old_ifs
read_stat "/proc/$root_pid" || unsupported
[ "$s_start" = "$root_start" ] && [ "$s_tty" = "$root_tty" ] && [ "$s_sid" = "$root_sid" ] && [ "$s_tpgid" = "$root_foreground" ] || unsupported
case "$s_state" in Z|X) unsupported ;; esac
printf 'capability=linux-proc\n'
printf '%s' "$rows" | awk '
NF == 7 {{
  provider[$2]=$1; start[$2]=$3; foreground[$2]=$4; parent[$2]=$5; group[$2]=$6; role[$2]=$7
}}
END {{
  for (pid in provider) {{
    p=parent[pid]
    if (role[p] == "launcher" && role[pid] == "cli" && provider[p] == provider[pid] && group[p] == group[pid]) children[p]++
  }}
  count=0
  for (pid in provider) {{
    p=parent[pid]
    if (role[pid] == "launcher" && children[pid] > 1) continue
    if (role[p] == "launcher" && role[pid] == "cli" && provider[p] == provider[pid] && group[p] == group[pid] && children[p] == 1) continue
    if (++count > {AGENT_PROCESS_CAP}) {{ print "truncated=1"; exit }}
    printf "agent\t%s\t%s\t%s\t%s\n", provider[pid], pid, start[pid], foreground[pid]
  }}
}}' || exit 1
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
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nagent\tclaude\t42\t900\tbackground\nagent\tcodex\t7\t100\tforeground\nend\n",
        )
        .unwrap();
        assert_eq!(supported.0, RemoteAgentCapability::LinuxProc);
        assert_eq!(supported.1.len(), 2);
        assert_eq!(supported.1[0].provider, RemoteAgentProvider::Codex);
        assert_eq!(supported.1[1].pid, 42);
        assert!(supported.1[0].foreground);
        assert!(!supported.1[1].foreground);

        assert_eq!(
            parse_inventory(b"mini-term-agent-inventory-v2\ncapability=unsupported\nend\n")
                .unwrap(),
            (RemoteAgentCapability::Unsupported, Vec::new())
        );
    }

    #[test]
    fn parser_rejects_ambiguous_or_unbounded_rows() {
        for invalid in [
            b"capability=linux-proc\nend\n".as_slice(),
            b"mini-term-agent-inventory-v1\ncapability=linux-proc\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nagent\tunknown\t1\t2\tforeground\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nagent\tclaude\t0\t2\tforeground\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nagent\tclaude\t1\t2\tforeground\nagent\tcodex\t1\t2\tforeground\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\ntruncated=1\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=unsupported\nagent\tclaude\t1\t2\tforeground\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nend\nextra\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nagent\tclaude\t1\t2\tunknown\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nagent\tclaude\t1\t2\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nagent\tclaude\t1\t2\tforeground\textra\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\ninvalid=1\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nagent\tcodex\t4294967296\t1\tforeground\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nagent\tcodex\t1\t18446744073709551616\tforeground\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nagent\tcodex\t1\t0\tforeground\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nend\nend\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nend\r\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nend\0\n",
            b"mini-term-agent-inventory-v2\ncapability=linux-proc\nend",
        ] {
            assert!(parse_inventory(invalid).is_err(), "accepted {invalid:?}");
        }
        assert!(parse_inventory(&[0xff]).is_err());
        assert!(parse_inventory(&vec![b'x'; AGENT_OUTPUT_CAP_BYTES + 1]).is_err());
        let too_many = format!("{INVENTORY_HEADER}\ncapability=linux-proc\n{}end\n", (1..=65).map(|pid| format!("agent\tcodex\t{pid}\t1\tforeground\n")).collect::<String>());
        assert!(parse_inventory(too_many.as_bytes()).is_err());
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
        assert!(command.contains("MINITERM_MANAGED_ROOT_START_TICKS"));
        assert!(command.contains("owned_ancestry"));
        assert!(command.contains("root_foreground"));
        assert!(!command.contains("printf '%s' \"$env_lines\""));
        assert!(!command.contains("printf '%s' \"$args\""));
    }

    #[cfg(target_os = "linux")]
    mod generated_probe {
        use super::*;
        use std::io::Read;
        use std::process::{Command, Stdio};
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        const TIMEOUT: Duration = Duration::from_secs(90);

        fn fixture(scenario: &str, route: &RemoteAgentRoute, mismatches: &[String]) -> (RemoteAgentCapability, Vec<RemoteAgentProcess>) {
            assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"), "process fixtures are Actions-only");
            let mut child = Command::new("python3")
                    .args(["-c", include_str!("agent_probe_tests.py"), scenario, &build_probe_command(route)])
                    .args(mismatches)
                    .env_clear()
                    .env("PATH", "/usr/bin:/bin")
                    .env("GITHUB_ACTIONS", "true")
                    .env("MINITERM_AGENT_PROTOCOL_VERSION", route.protocol_version.to_string())
                    .env("MINITERM_EXECUTION_HOST_ID", route.execution_host_id.as_str())
                    .env("MINITERM_WORKTREE_ID", route.worktree_id.as_str())
                    .env("MINITERM_TAB_ID", route.tab_id.as_str())
                    .env("MINITERM_PANE_KEY", route.pane_key.as_str())
                    .env("MINITERM_TERMINAL_SESSION_ID", route.terminal_session_id.as_str())
                    .env("MINITERM_TERMINAL_INCARNATION_ID", route.terminal_incarnation_id.as_str())
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap();
            let stdout = child.stdout.take().unwrap();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let result = stdout.take((AGENT_OUTPUT_CAP_BYTES + 1) as u64).read_to_end(&mut bytes);
                let _ = tx.send(result.map(|_| bytes));
            });
            let deadline = Instant::now() + TIMEOUT;
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= deadline {
                    // The helper owns its session and cleans up on TERM; never
                    // send a signal to any process not created by this fixture.
                    let _ = Command::new("kill").args(["-TERM", &child.id().to_string()]).status();
                    let cleanup_deadline = Instant::now() + Duration::from_secs(30);
                    while child.try_wait().unwrap().is_none() {
                        if Instant::now() >= cleanup_deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    panic!("generated probe fixture timed out");
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            assert!(status.success(), "generated probe fixture failed: {scenario}");
            let bytes = rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
            parse_inventory(&bytes).unwrap()
        }

        #[test]
        fn generated_command_requires_exact_route_and_managed_ancestry() {
            let route = route();
            let mut mismatches = Vec::new();
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
                mismatches.push(build_probe_command(&mismatch));
            }
            let (capability, found) = fixture("owned", &route, &mismatches);
            assert_eq!(capability, RemoteAgentCapability::LinuxProc);
            assert_eq!(found.len(), 5);
            assert!(found.iter().all(|process| process.foreground && process.start_ticks > 0));
        }

        #[test]
        fn generated_command_excludes_copied_routes_helpers_and_wildcard_arguments() {
            for scenario in ["external-only", "helpers", "empty-argv-helper", "after-exit"] {
                let (capability, found) = fixture(scenario, &route(), &[]);
                assert_eq!(capability, RemoteAgentCapability::LinuxProc);
                assert!(found.is_empty());
            }
        }

        #[test]
        fn generated_command_checks_each_candidate_route_beneath_a_valid_root() {
            let (capability, found) = fixture("candidate-route-mismatches", &route(), &[]);
            assert_eq!(capability, RemoteAgentCapability::LinuxProc);
            assert_eq!(found.len(), 1);
            assert!(found[0].foreground);
        }

        #[test]
        fn generated_command_collapses_only_positive_launcher_chains() {
            for scenario in ["launcher", "independent-descendants", "multiple-launcher-children"] {
                let (capability, found) = fixture(scenario, &route(), &[]);
                assert_eq!(capability, RemoteAgentCapability::LinuxProc);
                assert_eq!(found.len(), 2, "{scenario}");
            }
        }

        #[test]
        fn generated_command_retains_owned_background_processes() {
            let (capability, found) = fixture("background", &route(), &[]);
            assert_eq!(capability, RemoteAgentCapability::LinuxProc);
            assert_eq!(found.len(), 2);
            assert_eq!(found.iter().filter(|process| process.foreground).count(), 1);
        }

        #[test]
        fn generated_command_cannot_report_unsupported_ownership_as_empty() {
            for scenario in ["unmanaged", "old-root", "missing-tty", "missing-tools"] {
                let (capability, found) = fixture(scenario, &route(), &[]);
                assert_eq!(capability, RemoteAgentCapability::Unsupported, "{scenario}");
                assert!(found.is_empty());
            }
        }

        #[test]
        fn generated_command_rejects_incomplete_reads_and_raced_identity() {
            for scenario in [
                "argv-read-error", "partial-env-read-error", "unterminated-argv",
                "oversized-argv", "oversized-env", "exec-race", "pid-reuse",
                "foreground-race", "exit-race",
            ] {
                let (capability, found) = fixture(scenario, &route(), &[]);
                assert_eq!(capability, RemoteAgentCapability::Unsupported, "{scenario}");
                assert!(found.is_empty());
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
