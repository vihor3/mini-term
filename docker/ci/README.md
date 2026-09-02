# Docker-only build and test environment

mini-term does not require or support a host Rust toolchain for repository
verification. Source is bind-mounted read-only into the container; Cargo's
registry and `target` live under the Docker-only cache directory
`~/.cache/mini-term/docker-ci` by default.

```bash
scripts/docker-ci.sh build
scripts/docker-ci.sh worktree
scripts/docker-ci.sh check
scripts/docker-ci.sh test
```

Run an individual command with:

```bash
scripts/docker-ci.sh run cargo test -p mt-project
```

Remove all Docker-only build artifacts with:

```bash
scripts/docker-ci.sh clean
```

The image uses Rust 1.95 and the Linux GUI development libraries installed by
GitHub Actions. Do not install Rust, Cargo, Clippy, rustfmt, or GPUI build
packages on the host for this repository.
