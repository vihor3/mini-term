"""Tasks-only host credential owner, executed with python3 -I -c, never installed."""

import json
import os
import re
import selectors
import signal
import subprocess
import sys
import time

SECRET_LIMIT = 4096
STDERR_LIMIT = 65536
REMOVED_ENV = (
    "GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN",
    "GH_HOST", "GH_REPO", "GH_DEBUG", "DEBUG", "GH_FORCE_TTY", "CLICOLOR_FORCE",
    "GH_BROWSER", "BROWSER", "SSH_ASKPASS", "GIT_ASKPASS", "BASH_ENV", "ENV",
    "SHELLOPTS", "BASHOPTS", "WSLENV",
)
cancelled = False

# Only these fixed diagnostics may leave discovery; raw account errors stay here.
DIAGNOSTICS = (
    ("gh: command not found", ("gh: command not found", "gh: not found", "gh: no such file or directory")),
    ("unknown flag: --user", ("unknown flag: --user", "unknown shorthand flag: 'u'")),
    ("unknown flag: --json", ("unknown flag: --json", "unknown json field", "unknown flag: --jq")),
    ("rate limit", ("rate limit", "http 429", "status code 429")),
    ("connection timed out", ("could not resolve host", "no such host", "network is unreachable",
        "connection refused", "connection reset", "connection timed out", "tls handshake timeout",
        "context deadline exceeded", "temporary failure in name resolution", "no route to host",
        "error connecting to", "i/o timeout")),
    ("credential store is unavailable", ("keyring is locked", "keyring access denied",
        "failed to unlock keyring", "cannot access keyring", "failed to open keyring",
        "credential store is unavailable", "secure storage is unavailable",
        "org.freedesktop.secrets", "interaction is not allowed")),
    ("no oauth token found", ("no oauth token found", "no token found for")),
    ("hostname mismatch", ("not a known github host", "hostname mismatch", "account mismatch",
        "does not match the authenticated account")),
    ("bad credentials", ("http 401", "status code 401", "bad credentials", "token is invalid",
        "token has been revoked", "authentication failed")),
    ("insufficient scopes", ("insufficient scopes", "missing required scope", "requires the `read:org` scope")),
    ("permission denied", ("http 403", "status code 403", "resource not accessible by personal access token",
        "resource not accessible by integration", "permission denied", "forbidden")),
    ("http 404", ("http 404", "status code 404", "could not resolve to", "not found")),
    ("not logged into", ("not logged into", "gh auth login")),
    ("unknown flag", ("unknown flag", "unknown shorthand flag", "unknown command")),
)


class Failure(Exception):
    pass


def mark_cancelled(_signum, _frame):
    global cancelled
    cancelled = True


def checkpoint(deadline):
    if cancelled:
        raise Failure("cancelled")
    if time.monotonic() >= deadline:
        raise Failure("timed-out")


def cleanup(child):
    try:
        os.killpg(child.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    except OSError:
        raise Failure("cleanup-failed") from None
    try:
        child.wait(timeout=1)
    except Exception:
        raise Failure("cleanup-failed") from None


def capture(args, env, deadline, cap):
    checkpoint(deadline)
    try:
        child = subprocess.Popen(
            ["gh"] + args, env=env, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
        )
    except FileNotFoundError:
        raise Failure("client-missing") from None
    except Exception:
        raise Failure("failed") from None
    stdout, stderr = bytearray(), bytearray()
    retired = False
    try:
        with selectors.DefaultSelector() as streams:
            streams.register(sys.stdin, selectors.EVENT_READ, None)
            for pipe, data, limit in ((child.stdout, stdout, cap), (child.stderr, stderr, STDERR_LIMIT)):
                os.set_blocking(pipe.fileno(), False)
                streams.register(pipe, selectors.EVENT_READ, (data, limit))
            while len(streams.get_map()) > 1 or child.poll() is None:
                checkpoint(deadline)
                if not retired and child.poll() is not None:
                    cleanup(child)
                    retired = True
                for key, _mask in streams.select(min(0.02, max(0, deadline - time.monotonic()))):
                    if key.data is None:
                        # EOF (lost client) and any cancellation byte have the same effect.
                        os.read(sys.stdin.fileno(), 1)
                        raise Failure("cancelled")
                    data, limit = key.data
                    chunk = os.read(key.fd, 8192)
                    if not chunk:
                        streams.unregister(key.fileobj)
                        key.fileobj.close()
                    elif len(data) + len(chunk) > limit:
                        raise Failure("malformed")
                    else:
                        data.extend(chunk)
        return stdout, stderr, child.returncode
    finally:
        if not retired:
            cleanup(child)
        child.stdout.close()
        child.stderr.close()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise Failure("malformed")
        result[key] = value
    return result


def safe_output(captured, secret):
    out, err, code = captured
    if secret and (secret in out or secret in err):
        raise Failure("unsafe-output")
    try:
        return {"status": "output", "stdout": out.decode("utf-8"),
                "stderr": err.decode("utf-8"), "exit_code": code}
    except UnicodeError:
        raise Failure("malformed") from None


def nonsecret_diagnostic(text):
    text = text.lower()
    for diagnostic, needles in DIAGNOSTICS:
        if any(needle in text for needle in needles):
            return diagnostic
    return "account request failed"


def discovery_output(captured, data):
    result = safe_output(captured, None)
    if result["exit_code"] != 0:
        result["stderr"] = nonsecret_diagnostic(result["stderr"] + "\n" + result["stdout"])
        result["stdout"] = ""
        return result
    if data[:2] != ["auth", "status"] or "--json" not in data:
        return result
    try:
        status = json.loads(result["stdout"], object_pairs_hook=unique_object)
        hosts = status["hosts"] if isinstance(status, dict) else None
        if not isinstance(hosts, dict) or len(hosts) > 1:
            raise Failure("malformed")
        projected = {}
        for host, rows in hosts.items():
            if not valid_host(host) or not isinstance(rows, list) or len(rows) > 64:
                raise Failure("malformed")
            projected[host] = []
            for row in rows:
                if not isinstance(row, dict):
                    raise Failure("malformed")
                login = row.get("login")
                if (not valid_host(row.get("host")) or not isinstance(login, str)
                        or len(login) > 39 or not re.fullmatch(r"[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*(?:_[A-Za-z0-9]+)?", login)
                        or type(row.get("active")) is not bool
                        or row.get("state") not in ("success", "error", "timeout")):
                    raise Failure("malformed")
                entry = {key: row[key] for key in ("host", "login", "active", "state")}
                if "error" in row:
                    if not isinstance(row["error"], str) or not row["error"]:
                        raise Failure("malformed")
                    entry["error"] = nonsecret_diagnostic(row["error"])
                if "tokenSource" in row:
                    if not isinstance(row["tokenSource"], str):
                        raise Failure("malformed")
                    if row["tokenSource"].endswith("_TOKEN"):
                        entry["tokenSource"] = "GH_TOKEN"
                projected[host].append(entry)
        result["stdout"] = json.dumps({"hosts": projected}, separators=(",", ":"))
        result["stderr"] = ""
        return result
    except (ValueError, KeyError, TypeError):
        raise Failure("malformed") from None


def valid_host(host):
    if not isinstance(host, str):
        return False
    host = host[:-1] if host.endswith(".") else host
    return len(host) <= 253 and all(
        len(label) <= 63 and re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?", label)
        for label in host.split(".")
    )


def proof_result(captured, expected, secret):
    result = safe_output(captured, secret)
    if result["exit_code"] != 0:
        return result
    try:
        identity = json.loads(result["stdout"], object_pairs_hook=unique_object)
        login = identity["login"] if isinstance(identity, dict) else None
    except Exception:
        raise Failure("malformed") from None
    if not isinstance(login, str) or not login.isascii():
        raise Failure("malformed")
    if login.lower() != expected:
        raise Failure("identity-mismatch")
    return result


def lookup_error(stderr):
    text = stderr.lower()
    if b"unknown flag: --user" in text or b"unknown shorthand flag: 'u'" in text:
        return "named-account-unsupported"
    if any(part in text for part in (
        b"keyring is locked", b"keyring access denied", b"failed to unlock keyring",
        b"cannot access keyring", b"failed to open keyring", b"credential store is unavailable",
        b"secure storage is unavailable", b"org.freedesktop.secrets", b"interaction is not allowed",
    )):
        return "credential-store-unavailable"
    return "credential-lookup-failed"


def main():
    if sys.version_info < (3, 8) or os.name != "posix":
        raise Failure("helper-unavailable")
    cwd, milliseconds, cap, host, lookup_login, expected, data, proof = sys.argv[1:]
    cap, milliseconds = int(cap), int(milliseconds)
    if not (1 <= cap <= 4 * 1024 * 1024 and 100 <= milliseconds <= 120000):
        raise Failure("malformed")
    deadline = time.monotonic() + milliseconds / 1000
    data, proof = json.loads(data), json.loads(proof)
    if not isinstance(data, list) or not all(isinstance(arg, str) for arg in data):
        raise Failure("malformed")
    os.chdir(cwd)
    os.set_blocking(sys.stdin.fileno(), False)
    for signum in (signal.SIGHUP, signal.SIGTERM, signal.SIGINT):
        signal.signal(signum, mark_cancelled)
    env = dict(os.environ)
    for key in REMOVED_ENV:
        env.pop(key, None)
    env.update(GH_PROMPT_DISABLED="1", GH_PAGER="cat", PAGER="cat",
               NO_COLOR="1", TERM="dumb", LC_ALL="C")
    if not host:
        result = discovery_output(capture(data, env, deadline, cap), data)
        checkpoint(deadline)
        return result
    out, err, code = capture(
        ["auth", "token", "--hostname", host, "--user", lookup_login],
        env, deadline, SECRET_LIMIT + 2,
    )
    if code != 0:
        raise Failure(lookup_error(err))
    if out.endswith(b"\n"):
        del out[-1:]
        if out.endswith(b"\r"):
            del out[-1:]
    if not out or len(out) > SECRET_LIMIT or any(byte < 33 or byte > 126 for byte in out):
        raise Failure("credential-lookup-failed")
    secret = out
    variable = "GH_TOKEN" if host == "github.com" or host.endswith(".ghe.com") else "GH_ENTERPRISE_TOKEN"
    env[variable] = secret.decode("ascii")
    try:
        before = proof_result(capture(proof, env, deadline, STDERR_LIMIT), expected, secret)
        if before["exit_code"] != 0 or data == proof:
            checkpoint(deadline)
            return before
        result = safe_output(capture(data, env, deadline, cap), secret)
        after = proof_result(capture(proof, env, deadline, STDERR_LIMIT), expected, secret)
        checkpoint(deadline)
        return result if after["exit_code"] == 0 else after
    finally:
        env.pop(variable, None)
        secret[:] = b"\0" * len(secret)


try:
    reply = main()
except Failure as failure:
    reply = {"status": failure.args[0]}
except BaseException:
    # No traceback, argv, credential, child stdout/stderr, or exception text.
    reply = {"status": "failed"}
sys.stdout.write(json.dumps(reply, ensure_ascii=False, separators=(",", ":")))
sys.stdout.flush()
