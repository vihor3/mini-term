# Implementation Plan

1. Verify the current release inputs, version, required binaries, and resource
   behavior.
2. Create a temporary Docker cross-build container using Rust 1.95,
   `cargo-xwin`, LLVM/lld, NSIS, Node, and PE inspection tools.
3. Run a minimal Windows-target build to expose target-specific dependency or
   resource failures before making source changes.
4. If required, correct only the Windows resource build boundary and repeat the
   Docker build.
5. Build and stage the main executable, sidecars, and portable ConPTY payload.
6. Run `makensis` against the existing installer definition and write the setup
   executable to `dist/`.
7. Validate PE machine types, resource metadata, ConPTY hashes/layout, installer
   contents, size, and SHA-256 entirely in Docker.
8. Run repository checks relevant to any changed source/build files in Docker.
9. Remove the temporary container and project-specific build/tool caches; verify
   the host still has no Rust toolchain or project `target/` directory.
10. Record results in the task, commit only task-owned source/task files, and
    report the artifact path plus the remaining Windows runtime-test limitation.
