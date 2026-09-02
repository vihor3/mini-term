# Windows installer build in Docker

## Goal

Produce an installable Windows x64 build of the current mini-term branch without
recreating a Rust, Cargo, Node, or compiler environment on the host. The result
must use the repository's existing Windows release layout so it can be handed to
the user directly.

## Background

- The repository's release workflow builds `x86_64-pc-windows-msvc` on a native
  Windows runner and packages it with `scripts/windows-installer.nsi`.
- The host has intentionally been cleaned of project build environments. All
  compilation, tests, and packaging must run inside Docker.
- The current workspace version is `1.2.2`. The working tree also contains
  unrelated Trellis/bootstrap changes that must not be reverted or bundled as
  source edits for this task.

## Requirements

- Build the root `mt-app` binary and all three sidecars for
  `x86_64-pc-windows-msvc` inside a Linux Docker container.
- Stage the pinned portable ConPTY payload using the existing repository script
  and preserve the release layout expected by the application and installer.
- Package the staged payload with the existing NSIS script as
  `dist/Mini-Term_1.2.2_x64-setup.exe`.
- Preserve the application executable's Windows icon and version resources. If
  the current host-gated build script prevents that during cross compilation,
  correct it within the Docker build path or post-inject equivalent resources
  without changing application runtime behavior.
- Validate that the installer and bundled executables are non-empty PE files,
  have the expected machine type where applicable, and record a SHA-256 digest.
- Keep all Cargo, Rust target, Windows SDK, compiler, test, and packaging caches
  in Docker-only storage under the project-specific cache root, then remove
  those caches after the final artifact is copied out.

## Out Of Scope

- Completing the remaining Orca-style UI/runtime parent task.
- Publishing a GitHub release, creating a tag, or signing the installer.
- Installing or launching the package on a physical Windows machine from this
  Linux host.
- Reformatting, staging, or reverting unrelated working-tree changes.

## Acceptance Criteria

- [x] `dist/Mini-Term_1.2.2_x64-setup.exe` exists and is non-empty.
- [x] `mini-term.exe`, `miniterm-hook.exe`, `mt-ssh-cli.exe`, and
  `mt-ssh-mcp.exe` are built for Windows x64 in Docker.
- [x] The portable ConPTY directory contains the pinned x64 DLL plus x64 and
  ARM64 `OpenConsole.exe` files and passes the repository's hash/machine checks.
- [x] NSIS successfully compiles the existing installer definition in Docker.
- [x] PE/resource inspection confirms the main executable contains the expected
  Mini-Term version metadata and an icon resource.
- [x] The final installer SHA-256 and file size are reported to the user.
- [x] Host `cargo`, `rustc`, project `target/`, and project-specific Docker build
  caches are absent after packaging.

## Notes

- The installer represents the current branch, not completion of the broader
  Orca UI/runtime feature set.
