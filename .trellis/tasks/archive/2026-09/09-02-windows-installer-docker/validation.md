# Validation

Date: 2026-09-02

## Build Environment

- Base image: `mini-term-ci:rust-1.95`
- Rust: `rustc 1.95.0 (59807616e 2026-04-14)`
- Cross compiler: `cargo-xwin 0.19.2`
- Target: `x86_64-pc-windows-msvc`
- Shader compiler: Microsoft Windows SDK 10.0.22621 `fxc.exe`, executed through
  Wine inside the build container
- Installer compiler: NSIS 3.08

## Build Results

- `miniterm-hook.exe`: Windows x64 PE, console subsystem
- `mt-ssh-cli.exe`: Windows x64 PE, console subsystem
- `mt-ssh-mcp.exe`: Windows x64 PE, console subsystem
- `mini-term.exe`: Windows x64 PE, GUI subsystem
- GPUI release shaders were generated with the official Microsoft `fxc` after a
  temporary Docker-cache-only correction to GPUI 0.2.2's host/target build-script
  gate. No repository dependency source was changed.
- The main executable received icon, version, and GPUI DPI manifest resources in
  the Docker staging directory. Resource types 3, 14, 16, and 24 were present.

## Payload Validation

- NSIS compiled the existing `scripts/windows-installer.nsi` definition.
- 7-Zip recognized the output as `NSIS-3 Unicode` and extracted all expected
  payloads.
- Every extracted payload SHA-256 matched its staging source.
- Portable ConPTY hashes matched the constants in `scripts/stage-conpty.mjs`:
  - `conpty.dll`: `39fba2713e2495117b1591ae8c32a3b904bea7aa66069cf7815e2844c76d75d8`
  - `x64/OpenConsole.exe`: `b7fd936c2668b87b9ecf7b3366dc6568afc1c6f981874cba3e955a1c35cf8160`
  - `arm64/OpenConsole.exe`: `ed7622fd0d3bedc9ab9f122f5e58edf0def9e7999224f52dd395ba9f54edbe09`

## Final Artifact

- Path: `dist/Mini-Term_1.2.2_x64-setup.exe`
- Size: `16,926,968` bytes
- SHA-256: `0bc4cbf2d71b700e7094493dc056c7c4aef1974236153b983bef12011df55b20`
- Signature: unsigned, as expected for this local build

## Cleanup And Limits

- The temporary build container and project-specific Docker build cache were
  removed after the artifact was copied out.
- The host has no `cargo`, `rustc`, Rust home directories, or project `target/`.
- Runtime installation and launch were not exercised on a physical Windows
  machine from this Linux host.
