"""Exercise the generated production login command only in Linux Actions."""

import fcntl
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import termios
import time


assert os.environ.get("GITHUB_ACTIONS") == "true", "Actions-only login fixture"
command, remote_path, *pairs = sys.argv[1:]
assert len(pairs) == 14
expected_route = dict(zip(pairs[::2], pairs[1::2]))


def interrupted(_signum, _frame):
    raise SystemExit(1)


signal.signal(signal.SIGTERM, interrupted)

LOGIN_SHELL = """#!/bin/sh
[ "$#" -eq 1 ] && [ "$1" = -l ] || exit 21
exec "$MT_FIXTURE_PYTHON" -u "$MT_FIXTURE_REPORTER"
"""

REPORTER = r"""
import json, os, sys
from pathlib import Path
expected = json.loads(os.environ["MT_FIXTURE_EXPECTED_ROUTE"])
assert {key: os.environ.get(key) for key in expected} == expected, "route mismatch"
assert Path.cwd() == Path(os.environ["MT_FIXTURE_EXPECTED_CWD"]), "cwd mismatch"
assert os.environ["MINITERM_MANAGED_ROOT_PID"] == str(os.getpid()), "exec changed root PID"
stat = Path("/proc/self/stat").read_text().rsplit(") ", 1)[1].split()
assert int(stat[19]) > 0 and int(stat[4]) > 0, "missing root start or TTY"
assert os.environ["MINITERM_MANAGED_ROOT_START_TICKS"] == stat[19], "start mismatch"
assert os.environ["MINITERM_MANAGED_ROOT_TTY"] == stat[4], "TTY mismatch"
assert os.getsid(0) == os.getpid() == os.getpgrp(), "not the owned terminal session"
assert int(stat[5]) == os.getpgrp(), "not the foreground group"
print("login-ready", flush=True)
assert sys.stdin.readline(32) == "quit\n", "unexpected terminal input"
print("login-done", flush=True)
"""


def read_frame(master, expected):
    deadline = time.monotonic() + 10
    data = bytearray()
    while b"\n" not in data:
        remaining = deadline - time.monotonic()
        assert remaining > 0, "login readiness timeout"
        assert select.select([master], [], [], remaining)[0], "login readiness timeout"
        chunk = os.read(master, min(4096, 8193 - len(data)))
        assert chunk, "login closed before readiness"
        data.extend(chunk)
        assert len(data) <= 8192, "unexpected login output size"
    assert bytes(data) in [expected + b"\n", expected + b"\r\n"], "unexpected login frame"


with tempfile.TemporaryDirectory(prefix="mini-term-login-") as temp:
    directory = Path(temp)
    project = directory / remote_path
    project.mkdir()
    shell = directory / "shell 'with spaces'"
    shell.write_text(LOGIN_SHELL, encoding="utf-8")
    shell.chmod(0o700)
    reporter = directory / "reporter.py"
    reporter.write_text(REPORTER, encoding="utf-8")
    env = {
        "PATH": "/usr/bin:/bin", "HOME": temp, "SHELL": str(shell),
        "GITHUB_ACTIONS": "true", "MT_FIXTURE_PYTHON": sys.executable,
        "MT_FIXTURE_REPORTER": str(reporter),
        "MT_FIXTURE_EXPECTED_ROUTE": json.dumps(expected_route),
        "MT_FIXTURE_EXPECTED_CWD": str(project.resolve()),
        "MINITERM_MANAGED_ROOT_PID": "1",
        "MINITERM_MANAGED_ROOT_START_TICKS": "1",
        "MINITERM_MANAGED_ROOT_TTY": "1",
        **{key: "wrong-inherited-route" for key in expected_route},
    }
    master, slave = os.openpty()
    mode = termios.tcgetattr(slave)
    mode[3] &= ~termios.ECHO
    termios.tcsetattr(slave, termios.TCSANOW, mode)

    def owned_terminal():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    child = None
    try:
        child = subprocess.Popen(
            ["/bin/sh", "-c", command], cwd=directory, env=env,
            stdin=slave, stdout=slave, stderr=slave, preexec_fn=owned_terminal,
        )
        os.close(slave)
        slave = None
        read_frame(master, b"login-ready")
        # Inspect only the explicitly spawned child, never enumerate host /proc.
        stat = Path(f"/proc/{child.pid}/stat").read_text().rsplit(") ", 1)[1].split()
        assert int(stat[2]) == child.pid == int(stat[3]) == int(stat[5])
        assert int(stat[4]) > 0 and int(stat[19]) > 0
        os.write(master, b"quit\n")
        read_frame(master, b"login-done")
        assert child.wait(timeout=5) == 0, "login fixture failed"
    finally:
        try:
            if child is not None and child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=3)
        finally:
            if slave is not None:
                os.close(slave)
            os.close(master)
