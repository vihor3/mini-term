# Validation

Date: 2026-09-02

## Scope Review

- The final Trellis check reviewed `main.rs`, `overlay.rs`,
  `orca_sidebar.rs`, `workbench_area.rs`, and the file-tree click/rename path
  against the PRD, design, implementation plan, and `mt-app` specs.
- The Orca shell is the default. The legacy shell is reachable only when
  `MINI_TERM_LEGACY_SHELL` is explicitly `1`, `true`, or `yes`.
- The right context tab order is fixed as `Files / Git / Tasks / Sessions`.
- The Agents panel is a bounded floating live-activity overlay and does not
  replace the active workbench.
- Worktree activation re-enters the target project's remembered terminal or
  document page.
- A clean preview tab is replaced in place. Editing or double-clicking the tab
  promotes it; double-clicking a file-tree row opens rename instead.

## Docker Quality Checks

- Task-owned Rust files passed Docker `rustfmt --check`.
- `./scripts/docker-ci.sh check`: passed.
- `./scripts/docker-ci.sh clippy`: passed.
- Focused `mt-app` tests: 34 passed.
  - `orca_sidebar`: 6 passed.
  - `workbench_area`: 6 passed.
  - `file_tree::tests`: 19 passed.
  - `orca_shell_tests`: 3 passed.
- `git diff --check`: passed.
- The full workspace test run reached 800 passed and 1 failed in `mt-app`.
  The failure is the pre-existing, out-of-scope DnD normalized-path duplicate
  detection test: it expected `Duplicate` and received `Valid`.
- Strict workspace `-D warnings` remains blocked by pre-existing warnings in
  `mt-ai` and `mt-project`; no warning was reported in the task-owned files.

## Windows Build

- Base image: `mini-term-ci:rust-1.95`.
- Rust: `rustc 1.95.0 (59807616e 2026-04-14)`.
- Cross compiler: `cargo-xwin 0.19.2`.
- Target: `x86_64-pc-windows-msvc`.
- Shader compiler: Microsoft Windows SDK 10.0.22621 `fxc.exe`, run through
  Wine inside Docker.
- Installer compiler: NSIS 3.08.
- The three sidecars and `mt-app` release executable compiled successfully.
- Before packaging, the main PE contained the Orca-only markers
  `MINI_TERM_LEGACY_SHELL`, `Live activity`, and
  `Not available in this preview build`.
- The staged main executable is an x64 GUI PE. Resource types 3, 14, 16, and
  24 are present, with `ProductVersion=1.2.2-orca-20260902` and
  `FileVersion=1.2.2.902`.

## Installer Validation

- Path: `dist/Mini-Term_1.2.2-orca-20260902_x64-setup.exe`.
- Size: `16,985,374` bytes.
- SHA-256:
  `3668913f407eb3f4c97ee46593b509e6406b6ed56a07377fa50ba76283171896`.
- Signature: unsigned, as expected for this local build.
- 7-Zip recognized the artifact as an NSIS archive and extracted every
  expected payload.
- Every extracted executable and DLL matched its staging SHA-256.
- Main and sidecar PE machine values are x64; the bundled ARM64 OpenConsole is
  ARM64.
- Portable ConPTY hashes matched `scripts/stage-conpty.mjs`:
  - `conpty.dll`: `39fba2713e2495117b1591ae8c32a3b904bea7aa66069cf7815e2844c76d75d8`.
  - `x64/OpenConsole.exe`: `b7fd936c2668b87b9ecf7b3366dc6568afc1c6f981874cba3e955a1c35cf8160`.
  - `arm64/OpenConsole.exe`: `ed7622fd0d3bedc9ab9f122f5e58edf0def9e7999224f52dd395ba9f54edbe09`.

## UI Smoke Limit

- The actual Windows executable was launched under Wine/Xvfb with
  `MINI_TERM_LEGACY_SHELL` unset.
- Wine stopped before application initialization because its environment lacks
  `bcryptprimitives.dll` and `icuuc.dll`; no window was created, so no visual
  screenshot is claimed.
- Runtime launch on a real Windows host remains the final platform smoke check.

## Cleanup

- The temporary Windows build container, Windows build cache, and Docker CI
  Cargo/target cache were removed.
- The host has no `cargo`, `rustc`, project `target/`, `~/.cargo`, or
  `~/.rustup` state.
