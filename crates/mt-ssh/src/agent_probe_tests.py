"""Disposable Linux fixtures for agent.rs, executed only by GitHub Actions."""

import fcntl
import json
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import termios


assert os.environ.get("GITHUB_ACTIONS") == "true", "Actions-only process fixture"
SCENARIO, PROBE, *MISMATCHES = sys.argv[1:]
TIMEOUT = 10
FAULTS = {
    "argv-read-error", "partial-env-read-error", "unterminated-argv",
    "exec-race", "pid-reuse", "foreground-race", "exit-race",
}
UNSUPPORTED = {"unmanaged", "old-root", "missing-tty", "missing-tools",
               "oversized-argv", "oversized-env"} | FAULTS
OVERRIDES = FAULTS | {"empty-argv-helper"}


def interrupted(_signum, _frame):
    raise SystemExit(1)


signal.signal(signal.SIGTERM, interrupted)
signal.signal(signal.SIGHUP, signal.SIG_IGN)

# Own a new disposable session and controlling PTY. The copied-env decoy is a
# sibling of the managed root on this very same TTY, not a different device.
os.setsid()
master, slave = os.openpty()
if SCENARIO != "missing-tty":
    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

ROOT_BOOTSTRAP = r"""set -f
python=$1 script=$2 scenario=$3 directory=$4
IFS= read -r root_stat < /proc/$$/stat
root_tail=${root_stat##*) }
set -- $root_tail
root_tty=$5
shift 19
root_start=$1
if [ "$scenario" = old-root ]; then root_start=$((root_start + 1)); fi
exec env MINITERM_MANAGED_ROOT_PID="$$" MINITERM_MANAGED_ROOT_START_TICKS="$root_start" MINITERM_MANAGED_ROOT_TTY="$root_tty" "$python" -u "$script" "$scenario" "$directory"
"""

LAUNCHER = r"""
import json, os, signal, subprocess, sys
children = []
def interrupted(signum, frame):
    raise SystemExit(0)
signal.signal(signal.SIGTERM, interrupted)
try:
    for _ in range(int(sys.argv[2])):
        children.append(subprocess.Popen([sys.argv[1], "60"], stdin=subprocess.DEVNULL))
    print(json.dumps([child.pid for child in children]), flush=True)
    for child in children:
        child.wait()
finally:
    for child in children:
        if child.poll() is None:
            child.terminate()
    for child in children:
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
"""

ROOT = r"""
import json, os, select, signal, subprocess, sys
from pathlib import Path
scenario, directory = sys.argv[1:]
directory = Path(directory)
children, leaves, excluded = [], [], []
def interrupted(signum, frame):
    raise SystemExit(0)
signal.signal(signal.SIGTERM, interrupted)
def launch(argv, background=False, executable=None, wrapper=False, extra_env=None):
    child = subprocess.Popen(argv, stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE if wrapper else subprocess.DEVNULL,
        stderr=subprocess.DEVNULL, text=True, executable=executable,
        env=None if extra_env is None else dict(os.environ, **extra_env),
        preexec_fn=os.setpgrp if background else None)
    children.append(child)
    if wrapper:
        assert select.select([child.stdout], [], [], 10)[0], "launcher readiness timeout"
        line = child.stdout.readline(4096)
        assert line.endswith("\n"), "incomplete launcher readiness"
        leaves.extend(json.loads(line))
    return child
def stop():
    for child in children:
        if child.poll() is None:
            child.terminate()
    for child in children:
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
    children.clear()
def ready():
    print(json.dumps({"pids": [child.pid for child in children], "leaves": leaves, "excluded": excluded,
        "root": {key: value for key, value in os.environ.items() if key.startswith("MINITERM_MANAGED_ROOT_")}}), flush=True)
try:
    if scenario == "owned":
        for provider in ["claude", "codex", "opencode", "pi", "grok"]:
            launch([str(directory / provider), "60"])
    elif scenario in ["launcher", "multiple-launcher-children"]:
        launch([str(directory / "node"), str(directory / "@openai/codex/bin/codex.js"),
            str(directory / "codex"), "1" if scenario == "launcher" else "2"], wrapper=True)
        if scenario == "launcher":
            launch([str(directory / "codex"), "60"])
    elif scenario == "independent-descendants":
        launch([str(directory / "parent/codex"), str(directory / "@openai/codex/bin/codex.js"),
            str(directory / "codex"), "1"], wrapper=True)
    elif scenario == "background":
        launch([str(directory / "codex"), "60"], background=True)
        launch([str(directory / "claude"), "60"])
    elif scenario == "candidate-route-mismatches":
        for key in ["MINITERM_AGENT_PROTOCOL_VERSION", "MINITERM_EXECUTION_HOST_ID",
                    "MINITERM_WORKTREE_ID", "MINITERM_TAB_ID", "MINITERM_PANE_KEY",
                    "MINITERM_TERMINAL_SESSION_ID", "MINITERM_TERMINAL_INCARNATION_ID"]:
            child = launch([str(directory / "codex"), "60"], extra_env={key: "mismatch"})
            excluded.append(child.pid)
        launch([str(directory / "codex"), "60"])
    elif scenario == "oversized-argv":
        launch([str(directory / "parent/codex"), "-c", "import time; time.sleep(60)",
                "fixture-secret-sentinel-" + "x" * 4097])
    elif scenario == "oversized-env":
        launch([str(directory / "codex"), "60"],
               extra_env={"FIXTURE_SECRET": "fixture-secret-sentinel-" + "x" * 65537})
    elif scenario == "helpers":
        launch(["codex", "60"], executable="/bin/sleep")
        launch(["*", "60"], executable="/bin/sleep")
        launch([str(directory / "node"), str(directory / "@openai/codex/bin/helper.js")])
        launch([str(directory / "node"), "-c", "import time; time.sleep(60)", "codex"])
        launch([str(directory / "parent/codex"), "-c", "import time; time.sleep(60)", "--help"])
    elif scenario != "external-only":
        launch([str(directory / "codex"), "60"])
    ready()
    for line in sys.stdin:
        if line.strip() == "stop":
            stop()
            ready()
finally:
    stop()
"""

HEAD_FAULT = r"""#!/bin/sh
case "$3:$MT_FIXTURE_FAULT" in
  "/proc/$MT_FIXTURE_TARGET/cmdline:argv-read-error") exit 1 ;;
  "/proc/$MT_FIXTURE_TARGET/environ:partial-env-read-error") /usr/bin/head "$@"; exit 1 ;;
  "/proc/$MT_FIXTURE_TARGET/cmdline:unterminated-argv") printf 'codex\00060'; exit 0 ;;
  "/proc/$MT_FIXTURE_TARGET/cmdline:empty-argv-helper") printf 'codex\000-c\000\000--help\000'; exit 0 ;;
  "/proc/$MT_FIXTURE_TARGET/cmdline:exit-race") kill -TERM "$MT_FIXTURE_TARGET" ;;
  "/proc/$MT_FIXTURE_TARGET/stat:pid-reuse"|"/proc/$MT_FIXTURE_ROOT/stat:foreground-race")
    count=0
    if [ -f "$MT_FIXTURE_COUNT" ]; then read -r count < "$MT_FIXTURE_COUNT"; fi
    count=$((count + 1))
    printf '%s\n' "$count" > "$MT_FIXTURE_COUNT"
    if [ "$MT_FIXTURE_FAULT" = pid-reuse ] && [ "$count" -ge 3 ]; then
      /usr/bin/head "$@" | /usr/bin/awk '{$22=$22+1; print}'
      exit 0
    fi
    if [ "$MT_FIXTURE_FAULT" = foreground-race ] && [ "$count" -ge 2 ]; then
      /usr/bin/head "$@" | /usr/bin/awk '{$8=$8+1; print}'
      exit 0
    fi
    ;;
esac
exec /usr/bin/head "$@"
"""

READLINK_FAULT = r"""#!/bin/sh
if [ "$1" = "/proc/$MT_FIXTURE_TARGET/exe" ] && [ "$MT_FIXTURE_FAULT" = exec-race ]; then
  if [ -f "$MT_FIXTURE_COUNT" ]; then printf '/bin/sleep\n'; exit 0; fi
  printf '1\n' > "$MT_FIXTURE_COUNT"
fi
exec /usr/bin/readlink "$@"
"""


def ready(root):
    assert select.select([root.stdout], [], [], TIMEOUT)[0], "root readiness timeout"
    line = root.stdout.readline(4096)
    assert line.endswith("\n"), "incomplete root readiness"
    return json.loads(line)


def inspect(command=PROBE, path="/usr/bin:/bin", extra_env=None):
    result = subprocess.run(
        ["/bin/sh", "-c", command],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=dict({"PATH": path}, **(extra_env or {})),
        timeout=TIMEOUT,
        check=True,
    )
    assert not result.stderr, "probe leaked diagnostic output"
    assert len(result.stdout) <= 16 * 1024
    assert b"fixture-secret-sentinel-" not in result.stdout
    lines = result.stdout.decode("utf-8").splitlines()
    assert lines[0] == "mini-term-agent-inventory-v2"
    assert lines[-1] == "end"
    rows = []
    for line in lines[2:-1]:
        fields = line.split("\t")
        assert len(fields) == 5 and fields[0] == "agent", "malformed probe row"
        rows.append(fields)
    return result.stdout, lines[1], rows


def terminate_owned(child):
    if child.poll() is None:
        child.terminate()
    try:
        child.wait(timeout=TIMEOUT)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait(timeout=TIMEOUT)


with tempfile.TemporaryDirectory(prefix="mini-term-owned-agent-probe-") as temp:
    directory = Path(temp)
    for provider in ["claude", "codex", "opencode", "pi", "grok"]:
        shutil.copy2("/bin/sleep", directory / provider)
    # Real disposable executables, not argv0-only provider impersonation. A
    # Python interpreter named node runs an exact entrypoint-shaped fixture;
    # there is no dependency on a user's Node install or an actual Agent CLI.
    shutil.copy2(sys.executable, directory / "node")
    (directory / "parent").mkdir()
    shutil.copy2(sys.executable, directory / "parent/codex")
    scripts = directory / "@openai/codex/bin"
    scripts.mkdir(parents=True)
    (scripts / "codex.js").write_text(LAUNCHER, encoding="utf-8")
    (scripts / "helper.js").write_text("import time; time.sleep(60)", encoding="utf-8")
    root_script = directory / "managed_root.py"
    root_script.write_text(ROOT, encoding="utf-8")
    env = dict(os.environ, PYTHONHOME=sys.base_prefix)
    argv = [sys.executable, "-u", str(root_script), SCENARIO, str(directory)]
    if SCENARIO != "unmanaged":
        argv = ["/bin/sh", "-c", ROOT_BOOTSTRAP, "mini-term-fixture", sys.executable,
                str(root_script), SCENARIO, str(directory)]
    root = subprocess.Popen(argv, env=env, cwd=directory, stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    external = None
    try:
        facts = ready(root)
        # Copy even the true root facts onto a sibling on the same session/TTY.
        # Route environment, cwd, provider, and TTY alone must still not qualify.
        external = subprocess.Popen([str(directory / "codex"), "60"],
                                    env=dict(env, **facts["root"]), cwd=directory,
                                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
        if SCENARIO == "missing-tools":
            capture, capability, rows = inspect(path="/no-fixture-tools")
        elif SCENARIO in OVERRIDES:
            tools = directory / "fault-tools"
            tools.mkdir()
            for name, source in [("head", HEAD_FAULT), ("readlink", READLINK_FAULT)]:
                script = tools / name
                script.write_text(source, encoding="utf-8")
                script.chmod(0o700)
            capture, capability, rows = inspect(path=str(tools) + ":/usr/bin:/bin", extra_env={
                "MT_FIXTURE_FAULT": SCENARIO, "MT_FIXTURE_TARGET": str(facts["pids"][0]),
                "MT_FIXTURE_ROOT": str(root.pid), "MT_FIXTURE_COUNT": str(directory / "reads"),
            })
        else:
            capture, capability, rows = inspect()
        if SCENARIO in UNSUPPORTED:
            assert capability == "capability=unsupported" and not rows
        else:
            assert capability == "capability=linux-proc"
            pids = {int(row[2]) for row in rows}
            assert external.pid not in pids, "copied environment escaped ancestry gate"
            assert all(int(row[3]) > 0 for row in rows)
            expected = set(facts["pids"]) - set(facts["excluded"])
            if SCENARIO == "multiple-launcher-children":
                expected = set(facts["leaves"])
            elif SCENARIO == "independent-descendants":
                expected.update(facts["leaves"])
            elif SCENARIO in ["helpers", "empty-argv-helper"]:
                expected = set()
            assert pids == expected, "logical process identity mismatch"
            if SCENARIO not in OVERRIDES:
                again, next_capability, next_rows = inspect()
                assert next_capability == capability and sorted(next_rows) == sorted(rows)
            if SCENARIO == "background":
                assert sum(row[4] == "foreground" for row in rows) == 1
            else:
                assert all(row[4] == "foreground" for row in rows)
            for mismatch in MISMATCHES:
                _, mismatch_capability, mismatch_rows = inspect(mismatch)
                assert mismatch_capability == "capability=unsupported" and not mismatch_rows
            if SCENARIO == "after-exit":
                root.stdin.write("stop\n")
                root.stdin.flush()
                assert not ready(root)["pids"]
                for _ in range(2):
                    capture, capability, rows = inspect()
                    assert capability == "capability=linux-proc" and not rows
        sys.stdout.buffer.write(capture)
        sys.stdout.buffer.flush()
    finally:
        try:
            if external is not None:
                terminate_owned(external)
        finally:
            root.stdin.close()
            try:
                root.wait(timeout=TIMEOUT)
            except subprocess.TimeoutExpired:
                terminate_owned(root)
            assert root.returncode == 0, "owned fixture root failed"
os.close(slave)
os.close(master)
