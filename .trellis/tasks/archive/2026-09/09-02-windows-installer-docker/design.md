# Design

## Boundary

The existing Windows release contract remains authoritative: MSVC target,
release-profile binaries beside their sidecars, portable ConPTY in a sibling
directory, and the current NSIS definition. This task replaces only the native
Windows build host with a Linux Docker cross-build environment.

## Build Flow

1. Start from the existing Rust 1.95 Docker image and install container-local
   LLVM linker/resource tools plus NSIS.
2. Install `cargo-xwin` into a project-specific Docker cache and use its Windows
   SDK/CRT sysroot to build the root workspace and independent sidecar workspace
   for `x86_64-pc-windows-msvc`.
3. Copy the four PE binaries into one staging directory and run the existing
   ConPTY staging code against that directory.
4. Compile `scripts/windows-installer.nsi` with Linux `makensis`, passing the same
   version, source, icon, and output definitions used by the release workflow.
5. Inspect the PE headers/resources and installer contents from inside Docker,
   then copy only the final setup executable to `dist/`.

## Resource Compatibility

`crates/mt-app/build.rs` currently compiles `winresource` only when the build
script host is Windows. A Linux-hosted MSVC cross build therefore cannot embed
the icon/version data. The preferred correction is to gate execution by
`CARGO_CFG_TARGET_OS=windows` and make the build dependency available to the
Linux host, provided the crate can invoke the cross resource compiler supplied
by the xwin toolchain. If that path is unsupported, resource injection will be
performed as a container-only post-build step without changing runtime code.

## Operational Safety

- Source is mounted read-only for exploratory builds; writable build outputs and
  tool caches live under `/home/leo/.cache/mini-term/windows-build`.
- Any source fix is made narrowly and verified by a fresh Docker build.
- The final artifact is unsigned because no signing identity is configured.
- A failed cross build leaves the native Windows GitHub Actions release path
  unchanged; rollback is removal of the narrow packaging/resource change.
