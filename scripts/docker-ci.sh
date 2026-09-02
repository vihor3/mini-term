#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_ROOT="${MINI_TERM_DOCKER_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/mini-term/docker-ci}"
export MINI_TERM_DOCKER_CACHE_DIR="$CACHE_ROOT"
export LOCAL_UID="${LOCAL_UID:-$(id -u)}"
export LOCAL_GID="${LOCAL_GID:-$(id -g)}"

COMPOSE=(docker compose --project-directory "$ROOT_DIR" -f "$ROOT_DIR/docker-compose.ci.yml")

ensure_runtime() {
    command -v docker >/dev/null 2>&1 || {
        echo "docker is required; host Rust is intentionally unsupported" >&2
        exit 1
    }
    docker info >/dev/null 2>&1 || {
        echo "docker daemon is not available" >&2
        exit 1
    }
    mkdir -p "$CACHE_ROOT/cargo" "$CACHE_ROOT/target"
}

run_ci() {
    ensure_runtime
    "${COMPOSE[@]}" run --rm --no-deps ci "$@"
}

usage() {
    cat <<'EOF'
Usage: scripts/docker-ci.sh <command> [args...]

Commands:
  build              Build/update the Docker CI image.
  check              cargo check --workspace --all-targets.
  test               cargo test --workspace --all-targets.
  worktree           Run the Worktree Catalog V2 focused test/check suite.
  clippy             Run Clippy for mt-project and mt-app.
  fmt <base-sha>     Run the changed-line rustfmt gate for a committed diff.
  run <command...>   Run an arbitrary command in the CI container.
  shell              Open a shell in the CI container.
  clean              Remove the Docker-only Cargo/target cache.
EOF
}

command_name="${1:-}"
case "$command_name" in
    build)
        ensure_runtime
        "${COMPOSE[@]}" build ci
        ;;
    check)
        run_ci cargo check --workspace --all-targets
        ;;
    test)
        run_ci cargo test --workspace --all-targets
        ;;
    worktree)
        run_ci bash -c '
            set -euo pipefail
            cargo test -p mt-project --no-fail-fast
            cargo check -p mt-app --tests
            cargo test -p mt-app git_worktree --no-fail-fast
            cargo test -p mt-app project_list --no-fail-fast
            cargo clippy --no-deps -p mt-project -p mt-app --all-targets --message-format=short
        '
        ;;
    clippy)
        run_ci cargo clippy --no-deps -p mt-project -p mt-app --all-targets --message-format=short
        ;;
    fmt)
        base_sha="${2:-}"
        if [[ -z "$base_sha" ]]; then
            echo "fmt requires a committed diff base SHA" >&2
            exit 2
        fi
        run_ci node .github/scripts/check_changed_rustfmt.mjs "$base_sha"
        ;;
    run)
        shift
        if [[ "$#" -eq 0 ]]; then
            echo "run requires a command" >&2
            exit 2
        fi
        run_ci "$@"
        ;;
    shell)
        run_ci bash
        ;;
    clean)
        ensure_runtime
        "${COMPOSE[@]}" down --remove-orphans
        python3 - "$CACHE_ROOT" <<'PY_CLEAN_CACHE'
from pathlib import Path
import shutil
import sys

root = Path(sys.argv[1]).resolve()
if root.name != 'docker-ci' or root.parent.name != 'mini-term':
    raise SystemExit(f'refusing to remove unexpected cache path: {root}')
if root.exists():
    shutil.rmtree(root)
    print(f'removed {root}')
PY_CLEAN_CACHE
        ;;
    -h|--help|help|"")
        usage
        ;;
    *)
        echo "unknown command: $command_name" >&2
        usage >&2
        exit 2
        ;;
esac
