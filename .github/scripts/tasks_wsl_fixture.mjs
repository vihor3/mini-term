import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const ROOTFS_URL = "https://cloud-images.ubuntu.com/wsl/jammy/current/ubuntu-jammy-wsl-amd64-ubuntu22.04lts.rootfs.tar.gz";
const ROOTFS_SHA256 = "1483cc5c1dce13064f774834cbffdff226559fd522a67a381a8ea77d63fb4109";
const TEST = "tasks_account_executor::tests::tasks_account_executor_wsl_sentinels_cleanup_and_foreground_host";
const mode = process.argv[2];
const env = process.env;

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

requireCondition(env.GITHUB_ACTIONS === "true", "This fixture runs only in GitHub Actions");
for (const key of ["GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"]) {
  requireCondition(/^[0-9]+$/.test(env[key] ?? ""), `Invalid ${key}`);
}
requireCondition(/^[a-f0-9]{40}$/i.test(env.GITHUB_SHA ?? ""), "Invalid GITHUB_SHA");
requireCondition(/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(env.GITHUB_REPOSITORY ?? ""), "Invalid repository");
requireCondition(env.RUNNER_TEMP && env.GITHUB_WORKSPACE, "Missing Actions paths");

const temp = fs.realpathSync(env.RUNNER_TEMP);
const workspace = fs.realpathSync(env.GITHUB_WORKSPACE);
const suffix = `${env.GITHUB_RUN_ID}-${env.GITHUB_RUN_ATTEMPT}`;
const distro = `mt-tasks-${suffix}`;
const artifacts = path.join(temp, `mt-tasks-rootfs-${suffix}`);
const statePath = path.join(temp, `mt-tasks-wsl-${suffix}.json`);
const installPath = path.join(temp, `mt-tasks-wsl-${suffix}`);
const owner = {
  schema: 1,
  kind: "mini-term-tasks-wsl",
  run_id: env.GITHUB_RUN_ID,
  run_attempt: env.GITHUB_RUN_ATTEMPT,
  repository: env.GITHUB_REPOSITORY,
  sha: env.GITHUB_SHA,
};

function run(command, args, { timeout = 120_000, ...options } = {}) {
  const result = spawnSync(command, args, {
    cwd: workspace,
    timeout,
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
    ...options,
  });
  if (result.error || result.status !== 0) {
    // Credential fixtures deliberately produce hostile output; setup errors
    // must not accidentally become a raw output channel for a failed request.
    throw new Error(`${command} failed (${result.error?.code ?? result.status ?? "signal"})`);
  }
  return result.stdout;
}

async function sha256(file) {
  const hash = createHash("sha256");
  for await (const chunk of fs.createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}

function readJson(file, maxBytes = 8192) {
  requireCondition(fs.statSync(file).isFile() && fs.statSync(file).size <= maxBytes, "Invalid fixture metadata size");
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function sameOwner(value) {
  return value && Object.keys(value).length === Object.keys(owner).length
    && Object.entries(owner).every(([key, expected]) => value[key] === expected);
}

function listDistros() {
  const raw = run("wsl.exe", ["--list", "--all", "--quiet"]);
  const text = new TextDecoder(raw.includes(0) ? "utf-16le" : "utf-8", { fatal: true }).decode(raw);
  return text.replace(/^\uFEFF/, "").split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
}

function wsl(args) {
  return run("wsl.exe", ["--distribution", distro, "--user", "root", "--cd", "/", "--exec", ...args]);
}

function validateImportedOwner() {
  const raw = wsl(["/usr/bin/head", "-c", "8193", "/mini-term-fixture/owner.json"]);
  requireCondition(raw.length <= 8192, "Oversized WSL owner marker");
  requireCondition(sameOwner(JSON.parse(raw.toString("utf8"))), "WSL owner marker mismatch");
}

async function prepare() {
  requireCondition(process.platform === "linux" && process.arch === "x64", "Rootfs builder requires Linux x64");
  fs.mkdirSync(artifacts);
  const scratch = fs.mkdtempSync(path.join(temp, "mt-tasks-wsl-build-"));
  try {
    const archive = path.join(scratch, "base.tar.gz");
    const root = path.join(scratch, "root");
    const fixture = path.join(scratch, "gh");
    fs.mkdirSync(root);
    run("curl", ["--proto", "=https", "--tlsv1.2", "--fail", "--location", "--retry", "3", "--max-time", "600", "--output", archive, ROOTFS_URL], { timeout: 660_000 });
    requireCondition(await sha256(archive) === ROOTFS_SHA256, "Canonical rootfs checksum changed; review the pin explicitly");
    run("rustc", ["--edition=2024", "-C", "opt-level=0", path.join(workspace, "crates/mt-app/src/tasks_account_executor/gh_fixture.rs"), "-o", fixture]);
    const executable = fs.openSync(fixture, "r");
    try {
      const header = Buffer.alloc(20);
      requireCondition(fs.fstatSync(executable).isFile()
        && fs.readSync(executable, header, 0, header.length, 0) === header.length
        && header.subarray(0, 6).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1]))
        && header.readUInt16LE(18) === 62, "Synthetic gh fixture is not a Linux x64 ELF");
    } finally {
      fs.closeSync(executable);
    }
    run("sudo", ["tar", "--extract", "--gzip", "--file", archive, "--directory", root, "--no-same-owner"], { timeout: 300_000 });
    const fixtureRoot = path.join(root, "mini-term-fixture");
    run("sudo", ["install", "-d", "-m", "0755", path.join(root, "usr/local/bin"), fixtureRoot, path.join(fixtureRoot, "cases"), path.join(fixtureRoot, "home")]);
    run("sudo", ["install", "-m", "0755", fixture, path.join(root, "usr/local/bin/gh")]);
    run("sudo", ["install", "-m", "0755", path.join(workspace, "crates/mt-app/src/tasks_account_executor/wsl_fixture_launcher.sh"), path.join(root, "usr/local/bin/python3")]);
    for (const [name, value, destination] of [
      ["owner.json", JSON.stringify(owner) + "\n", path.join(fixtureRoot, "owner.json")],
      ["wsl.conf", "[automount]\nenabled=false\n[interop]\nenabled=false\nappendWindowsPath=false\n[user]\ndefault=root\n", path.join(root, "etc/wsl.conf")],
      ["environment", "PATH=/usr/local/bin:/usr/bin:/bin\nHOME=/mini-term-fixture/home\n", path.join(root, "etc/environment")],
    ]) {
      const source = path.join(scratch, name);
      fs.writeFileSync(source, value, { flag: "wx", mode: 0o600 });
      run("sudo", ["install", "-m", "0644", source, destination]);
    }
    for (const file of ["usr/bin/python3", "usr/bin/head", "usr/bin/sha256sum", "usr/bin/test", "usr/bin/touch", "bin/sh", "bin/cat", "bin/mkdir"]) {
      run("sudo", ["test", "-e", path.join(root, file)]);
    }
    const rootfs = path.join(artifacts, "rootfs.tar");
    run("sudo", ["tar", "--create", "--file", rootfs, "--directory", root, "."], { timeout: 300_000 });
    run("sudo", ["chown", `${process.getuid()}:${process.getgid()}`, rootfs]);
    fs.writeFileSync(path.join(artifacts, "manifest.json"), JSON.stringify({
      ...owner,
      kind: "mini-term-tasks-wsl-artifact",
      source_rootfs_url: ROOTFS_URL,
      source_rootfs_sha256: ROOTFS_SHA256,
      rootfs_sha256: await sha256(rootfs),
      gh_sha256: await sha256(fixture),
    }, null, 2) + "\n", { flag: "wx" });
    console.log(`Prepared same-run WSL fixture for ${env.GITHUB_SHA}`);
  } finally {
    run("sudo", ["rm", "-rf", "--", scratch]);
  }
}

async function importFixture() {
  requireCondition(process.platform === "win32", "WSL transport gate requires Windows");
  const manifest = readJson(path.join(artifacts, "manifest.json"));
  const { source_rootfs_url, source_rootfs_sha256, rootfs_sha256, gh_sha256, ...provenance } = manifest;
  requireCondition(sameOwner({ ...provenance, kind: "mini-term-tasks-wsl" }) && provenance.kind === "mini-term-tasks-wsl-artifact", "Artifact is not from this exact Actions run and commit");
  requireCondition(source_rootfs_url === ROOTFS_URL && source_rootfs_sha256 === ROOTFS_SHA256, "Artifact rootfs input mismatch");
  requireCondition(/^[a-f0-9]{64}$/.test(gh_sha256 ?? "") && /^[a-f0-9]{64}$/.test(rootfs_sha256 ?? ""), "Invalid fixture hashes");
  const rootfs = path.join(artifacts, "rootfs.tar");
  requireCondition(await sha256(rootfs) === rootfs_sha256, "Downloaded fixture checksum mismatch");
  requireCondition(!listDistros().some((name) => name.toLowerCase() === distro.toLowerCase()), "Refusing to reuse an existing distro");
  requireCondition(!fs.existsSync(statePath) && !fs.existsSync(installPath), "Fixture import is already owned");
  fs.mkdirSync(installPath);
  // Record ownership before dispatch so an interrupted import can be cleaned
  // without inspecting or unregistering any pre-existing distro.
  fs.writeFileSync(statePath, JSON.stringify({ owner, distro, installPath }), { flag: "wx" });
  run("wsl.exe", ["--import", distro, installPath, rootfs, "--version", "1"], { timeout: 600_000 });
  validateImportedOwner();
  requireCondition(env.GITHUB_ENV, "Missing Actions environment output");
  fs.appendFileSync(env.GITHUB_ENV, `MT_TEST_WSL_DISTRO=${distro}\nMT_TEST_WSL_MARKER=/mini-term-fixture/owner.json\nMT_TEST_WSL_GH_SHA256=${gh_sha256}\n`);
  console.log(`Imported isolated WSL 1 fixture ${distro}`);
}

function testFixture() {
  requireCondition(process.platform === "win32" && env.MT_TEST_WSL_DISTRO === distro, "Missing owned WSL test environment");
  validateImportedOwner();
  const args = ["test", "--locked", "--target", "x86_64-pc-windows-msvc", "-p", "mt-app", "--bin", "mini-term", TEST, "--"];
  const listing = spawnSync("cargo", [...args, "--list", "--ignored", "--exact"], {
    cwd: workspace,
    encoding: "utf8",
    timeout: 45 * 60_000,
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
  });
  process.stdout.write(listing.stdout ?? "");
  process.stderr.write(listing.stderr ?? "");
  requireCondition(!listing.error && listing.status === 0, "WSL test discovery/build failed");
  const tests = listing.stdout.split(/\r?\n/).filter((line) => line.endsWith(": test"));
  requireCondition(tests.length === 1 && tests[0] === `${TEST}: test`, "Exact WSL transport test was not discovered once");
  run("cargo", [...args, "--ignored", "--exact", "--test-threads=1"], { timeout: 15 * 60_000, stdio: "inherit" });
}

function cleanup() {
  requireCondition(process.platform === "win32", "WSL cleanup requires Windows");
  if (!fs.existsSync(statePath)) return;
  const state = readJson(statePath);
  requireCondition(sameOwner(state.owner) && state.distro === distro && state.installPath === installPath, "Refusing foreign WSL cleanup state");
  if (listDistros().some((name) => name.toLowerCase() === distro.toLowerCase())) {
    // This exact name was proven absent before the recorded import. Cleanup
    // remains valid after a partial import, where no guest marker is readable.
    try {
      run("wsl.exe", ["--terminate", distro], { timeout: 30_000 });
    } catch {
      console.warn("Owned WSL termination was not confirmed; still attempting unregister");
    }
    run("wsl.exe", ["--unregister", distro], { timeout: 300_000 });
    requireCondition(!listDistros().some((name) => name.toLowerCase() === distro.toLowerCase()),
      "Owned WSL distro remains registered; preserving cleanup state");
  }
  fs.rmSync(installPath, { recursive: true, force: true });
  fs.unlinkSync(statePath);
  console.log(`Cleaned only Actions-owned distro ${distro}`);
}

try {
  if (mode === "prepare") await prepare();
  else if (mode === "import") await importFixture();
  else if (mode === "test") testFixture();
  else if (mode === "cleanup") cleanup();
  else throw new Error("Expected prepare, import, test, or cleanup");
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
