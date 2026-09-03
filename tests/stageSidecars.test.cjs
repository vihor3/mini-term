const assert = require('node:assert/strict');
const { access, mkdtemp, mkdir, readFile, rm, writeFile } = require('node:fs/promises');
const { tmpdir } = require('node:os');
const { isAbsolute, join, relative, resolve } = require('node:path');
const { pathToFileURL } = require('node:url');
const test = require('node:test');

const repoRoot = resolve(__dirname, '..');
const windowsTarget = 'x86_64-pc-windows-msvc';
const sidecars = ['miniterm-hook', 'mt-ssh-mcp', 'mt-ssh-cli'];
const rootHelpers = ['mt-terminal-host'];
const stagingModuleUrl = pathToFileURL(
  join(repoRoot, 'scripts', 'stage-sidecars.mjs'),
).href;

async function loadStagingModule() {
  return import(stagingModuleUrl);
}

function minimalPe(machine) {
  const buffer = Buffer.alloc(0x100);
  buffer.writeUInt16LE(0x5a4d, 0);
  buffer.writeUInt32LE(0x80, 0x3c);
  buffer.write('PE\0\0', 0x80, 'ascii');
  buffer.writeUInt16LE(machine, 0x84);
  return buffer;
}

function pathIsWithin(root, candidate) {
  const rel = relative(root, candidate);
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
}

async function pathExists(path) {
  try {
    await access(path);
    return true;
  } catch (error) {
    if (error.code === 'ENOENT') return false;
    throw error;
  }
}

async function assertNoWorkspaceBuildDirs(workspaceDir) {
  for (const name of ['target', '.conpty-cache', 'dist', 'stage']) {
    assert.equal(await pathExists(join(workspaceDir, name)), false, `${name} should be absent`);
  }
}

async function writeWindowsBuildArtifacts(plan, { wrongMachineFor } = {}) {
  await Promise.all([
    mkdir(plan.sidecarBuiltDir, { recursive: true }),
    mkdir(plan.rootHelperBuiltDir, { recursive: true }),
  ]);
  const entries = [
    ...sidecars.map((name) => [name, plan.sidecarBuiltDir]),
    ...rootHelpers.map((name) => [name, plan.rootHelperBuiltDir]),
  ];
  await Promise.all(
    entries.map(([name, directory]) =>
      writeFile(
        join(directory, `${name}.exe`),
        minimalPe(name === wrongMachineFor ? 0xaa64 : 0x8664),
      ),
    ),
  );
}

async function writeStagedWindowsPayload(stageDir) {
  const x64Host = join(stageDir, 'portable-conpty', 'x64');
  const arm64Host = join(stageDir, 'portable-conpty', 'arm64');
  await Promise.all([
    mkdir(stageDir, { recursive: true }),
    mkdir(x64Host, { recursive: true }),
    mkdir(arm64Host, { recursive: true }),
  ]);
  await Promise.all([
    ...[...sidecars, ...rootHelpers, 'mini-term'].map((name) =>
      writeFile(join(stageDir, `${name}.exe`), minimalPe(0x8664)),
    ),
    writeFile(join(stageDir, 'portable-conpty', 'conpty.dll'), minimalPe(0x8664)),
    writeFile(join(x64Host, 'OpenConsole.exe'), minimalPe(0x8664)),
    writeFile(join(arm64Host, 'OpenConsole.exe'), minimalPe(0xaa64)),
  ]);
}

test('staging paths honor Cargo and stage roots while keeping host linking isolated', async () => {
  const { createStagingPlan } = await loadStagingModule();
  const workspaceDir = resolve(tmpdir(), 'mini-term-stage-plan-workspace');
  const cargoTargetDir = resolve(tmpdir(), 'mini-term-stage-plan-cache', 'target');
  const stageDir = resolve(tmpdir(), 'mini-term-stage-plan-cache', 'stage');
  const plan = createStagingPlan({
    args: ['--release', '--target', windowsTarget],
    cwd: workspaceDir,
    env: {
      CARGO_TARGET_DIR: cargoTargetDir,
      MINI_TERM_STAGE_DIR: stageDir,
    },
    triple: 'ignored-by-explicit-target',
  });

  assert.equal(plan.stageDir, stageDir);
  assert.equal(plan.sidecarTargetDir, join(cargoTargetDir, '.stage-sidecars', 'sidecars'));
  assert.equal(
    plan.rootHelperTargetDir,
    join(cargoTargetDir, '.stage-sidecars', 'root-helper'),
  );
  assert.equal(
    plan.sidecarBuiltDir,
    join(
      cargoTargetDir,
      '.stage-sidecars',
      'sidecars',
      'x86_64-pc-windows-msvc',
      'release',
    ),
  );
  assert.equal(
    plan.rootHelperBuiltDir,
    join(
      cargoTargetDir,
      '.stage-sidecars',
      'root-helper',
      'x86_64-pc-windows-msvc',
      'release',
    ),
  );
  assert.equal(
    plan.conptyCacheDir,
    join(cargoTargetDir, '.stage-sidecars', 'conpty-cache'),
  );
  assert.notEqual(plan.rootHelperBuiltDir, plan.stageDir);
  assert.ok(plan.sidecarCargoArgs.includes('--locked'));
  assert.ok(plan.rootCargoArgs.includes('--locked'));
  for (const path of [
    plan.sidecarTargetDir,
    plan.rootHelperTargetDir,
    plan.stageDir,
    plan.conptyCacheDir,
  ]) {
    assert.equal(pathIsWithin(workspaceDir, path), false, `${path} must stay outside source`);
  }
});

test('Windows dev host builds outside live target/debug before staging', async () => {
  const { createStagingPlan } = await loadStagingModule();
  const workspaceDir = resolve(tmpdir(), 'mini-term-stage-plan-default');
  const plan = createStagingPlan({
    cwd: workspaceDir,
    env: {},
    triple: windowsTarget,
  });

  assert.equal(plan.stageDir, join(workspaceDir, 'target', 'debug'));
  assert.equal(plan.sidecarBuiltDir, join(workspaceDir, 'sidecars', 'target', 'debug'));
  assert.equal(
    plan.rootHelperBuiltDir,
    join(workspaceDir, 'target', '.stage-sidecars', 'root-helper', 'debug'),
  );
  assert.notEqual(plan.rootHelperBuiltDir, plan.stageDir);
});

test('unsupported Windows targets fail before Cargo or workspace-local output', async (t) => {
  const tempRoot = await mkdtemp(join(tmpdir(), 'mini-term-stage-target-'));
  t.after(() => rm(tempRoot, { recursive: true, force: true }));
  const workspaceDir = join(tempRoot, 'workspace');
  await mkdir(workspaceDir);
  let cargoRuns = 0;
  const { stageSidecars } = await loadStagingModule();

  await assert.rejects(
    stageSidecars({
      args: ['--release', '--target', 'aarch64-pc-windows-msvc'],
      cwd: workspaceDir,
      env: {
        CARGO_TARGET_DIR: join(tempRoot, 'cache', 'target'),
        MINI_TERM_STAGE_DIR: join(tempRoot, 'cache', 'stage'),
      },
      execFile: () => {
        cargoRuns += 1;
      },
    }),
    /尚无匹配的 x64 sidecar\/便携 ConPTY 资源/,
  );

  assert.equal(cargoRuns, 0);
  await assertNoWorkspaceBuildDirs(workspaceDir);
});

test('dev copy failure keeps the live artifact while release remains strict', async (t) => {
  const tempRoot = await mkdtemp(join(tmpdir(), 'mini-term-stage-sidecars-'));
  t.after(() => rm(tempRoot, { recursive: true, force: true }));
  const source = join(tempRoot, 'new.exe');
  const destination = join(tempRoot, 'live.exe');
  await Promise.all([writeFile(source, 'new'), writeFile(destination, 'old')]);

  const lockedError = Object.assign(new Error('access denied'), { code: 'EPERM' });
  const warnings = [];
  const { copyStagedArtifact } = await loadStagingModule();
  const copied = copyStagedArtifact({
    from: source,
    staged: destination,
    release: false,
    copyFile: () => {
      throw lockedError;
    },
    log: () => {},
    warn: (message) => warnings.push(message),
  });

  assert.equal(copied, false);
  assert.equal(await readFile(destination, 'utf8'), 'old');
  assert.match(warnings[0], /EPERM/);
  assert.throws(
    () =>
      copyStagedArtifact({
        from: source,
        staged: destination,
        release: true,
        copyFile: () => {
          throw lockedError;
        },
      }),
    lockedError,
  );
});

test('wrong-architecture release output is rejected before copy and stale stage is cleaned', async (t) => {
  const tempRoot = await mkdtemp(join(tmpdir(), 'mini-term-stage-arch-'));
  t.after(() => rm(tempRoot, { recursive: true, force: true }));
  const workspaceDir = join(tempRoot, 'workspace');
  const targetDir = join(tempRoot, 'cache', 'target');
  const stageDir = join(tempRoot, 'cache', 'stage');
  await mkdir(workspaceDir);
  const args = ['--release', '--target', windowsTarget];
  const env = {
    CARGO_TARGET_DIR: targetDir,
    MINI_TERM_STAGE_DIR: stageDir,
  };
  const { createStagingPlan, stageSidecars } = await loadStagingModule();
  const plan = createStagingPlan({ args, cwd: workspaceDir, env, triple: windowsTarget });
  await writeWindowsBuildArtifacts(plan, { wrongMachineFor: 'mt-terminal-host' });
  await writeStagedWindowsPayload(stageDir);
  let conptyRuns = 0;

  await assert.rejects(
    stageSidecars({
      args,
      cwd: workspaceDir,
      env,
      execFile: () => {},
      stageConpty: async () => {
        conptyRuns += 1;
      },
      log: () => {},
      warn: () => {},
    }),
    /mt-terminal-host PE machine 不匹配/,
  );

  assert.equal(conptyRuns, 0);
  for (const name of [...sidecars, ...rootHelpers]) {
    assert.equal(await pathExists(join(stageDir, `${name}.exe`)), false);
  }
  assert.equal(await pathExists(join(stageDir, 'portable-conpty')), false);
  await assertNoWorkspaceBuildDirs(workspaceDir);
});

test('complete Windows release verification rejects a wrong-architecture executable', async (t) => {
  const tempRoot = await mkdtemp(join(tmpdir(), 'mini-term-stage-verify-'));
  t.after(() => rm(tempRoot, { recursive: true, force: true }));
  const workspaceDir = join(tempRoot, 'workspace');
  const stageDir = join(tempRoot, 'stage');
  await Promise.all([mkdir(workspaceDir), writeStagedWindowsPayload(stageDir)]);
  const args = ['--release', '--target', windowsTarget];
  const env = {
    CARGO_TARGET_DIR: join(tempRoot, 'target'),
    MINI_TERM_STAGE_DIR: stageDir,
  };
  const { createStagingPlan, validateStagedPayload } = await loadStagingModule();
  const plan = createStagingPlan({ args, cwd: workspaceDir, env, triple: windowsTarget });

  const artifacts = await validateStagedPayload(plan, {
    includeApp: true,
    verifyOfficialConptyHashes: false,
  });
  assert.equal(artifacts.length, 5);

  await writeFile(join(stageDir, 'mt-ssh-cli.exe'), minimalPe(0xaa64));
  await assert.rejects(
    validateStagedPayload(plan, {
      includeApp: true,
      verifyOfficialConptyHashes: false,
    }),
    /mt-ssh-cli PE machine 不匹配/,
  );
});

function cargoInvocationLines(source) {
  return source
    .split('\n')
    .map((line) => line.trim())
    .filter((line) =>
      /^(run:\s*|\$metadataJson\s*=\s*)?cargo (metadata|build|check|test|clippy)\b/.test(
        line,
      ),
    );
}

test('Actions own locked verification and Windows package evidence', async () => {
  const [ciWorkflow, releaseWorkflow, packageWorkflow, buildScript, verifyScript] =
    await Promise.all([
      readFile(join(repoRoot, '.github', 'workflows', 'ci.yml'), 'utf8'),
      readFile(join(repoRoot, '.github', 'workflows', 'release.yml'), 'utf8'),
      readFile(join(repoRoot, '.github', 'workflows', 'windows-package.yml'), 'utf8'),
      readFile(join(repoRoot, 'scripts', 'build-windows-installer.ps1'), 'utf8'),
      readFile(join(repoRoot, 'scripts', 'verify-windows-installer.ps1'), 'utf8'),
    ]);

  for (const [name, workflow] of [
    ['ci.yml', ciWorkflow],
    ['release.yml', releaseWorkflow],
    ['windows-package.yml', packageWorkflow],
  ]) {
    const cargoLines = cargoInvocationLines(workflow);
    assert.ok(cargoLines.length > 0, `${name} should contain Cargo verification`);
    assert.deepEqual(
      cargoLines.filter((line) => !line.includes('--locked')),
      [],
      `${name} contains an unlocked Cargo command`,
    );
    assert.match(
      workflow,
      /^\s*sidecars -> target\s*$/m,
      `${name} should cache the sidecar workspace target directory`,
    );
    assert.doesNotMatch(
      workflow,
      /sidecars -> sidecars\/target/,
      `${name} should not duplicate the sidecars path in its cache target`,
    );
  }

  assert.match(ciWorkflow, /RUSTFMT_PATCH_PATH: changed-rustfmt\.patch/);
  assert.match(ciWorkflow, /name: rustfmt-diagnostics-\$\{\{ github\.run_id \}\}/);
  assert.match(
    ciWorkflow,
    /path: \|\s*\n\s*changed-rustfmt\.patch\s*\n\s*full-rustfmt\.patch/,
  );
  assert.match(ciWorkflow, /uses: actions\/upload-artifact@v7/);
  assert.match(ciWorkflow, /cargo test --locked --workspace --all-targets/);
  assert.match(
    ciWorkflow,
    /cargo test --manifest-path sidecars\/Cargo\.toml --locked --all-targets/,
  );
  for (const packageName of [
    'mt-ai',
    'mt-app',
    'mt-config',
    'mt-github',
    'mt-identity',
    'mt-layout',
    'mt-project',
    'mt-pty',
    'mt-ssh',
    'mt-terminal',
    'mt-terminal-host',
    'mt-ui',
  ]) {
    assert.match(ciWorkflow, new RegExp(`-p ${packageName}(?=\\\\|\\s|$)`));
  }

  assert.match(
    packageWorkflow,
    /push:\s+branches:\s+- '\*\*'\s+tags-ignore:\s+- 'v\*'\s+paths:/,
  );
  assert.match(packageWorkflow, /- '\.github\/workflows\/windows-package\.yml'/);
  assert.match(packageWorkflow, /- 'crates\/\*\*'/);
  assert.match(packageWorkflow, /- 'sidecars\/\*\*'/);
  assert.match(packageWorkflow, /- 'scripts\/verify-windows-installer\.ps1'/);
  assert.match(packageWorkflow, /workflow_dispatch:\s+inputs:\s+version:/);
  assert.match(packageWorkflow, /\$baseVersion-ci\.\$\(\$env:RUN_NUMBER\)/);
  assert.match(packageWorkflow, /runs-on: windows-latest/);
  assert.match(
    packageWorkflow,
    /node --test tests\/stageSidecars\.test\.cjs tests\/conptyBundle\.test\.cjs/,
  );
  assert.match(
    packageWorkflow,
    /node scripts\/stage-sidecars\.mjs --release --target \$\{\{ env\.WINDOWS_TARGET \}\}/,
  );
  assert.match(packageWorkflow, /cargo build --locked --release --package mt-app/);
  assert.match(
    packageWorkflow,
    /node scripts\/stage-sidecars\.mjs --verify-only --release --target \$\{\{ env\.WINDOWS_TARGET \}\}/,
  );
  assert.match(packageWorkflow, /scripts\/build-windows-installer\.ps1/);
  assert.match(packageWorkflow, /scripts\/verify-windows-installer\.ps1/);
  assert.match(packageWorkflow, /uses: actions\/upload-artifact@v7/);
  assert.match(packageWorkflow, /dist\/windows-package-validation\.json/);
  assert.doesNotMatch(packageWorkflow, /softprops\/action-gh-release/);

  const packageStageVerify = packageWorkflow.indexOf('- name: Verify complete staged payload');
  const packageBuild = packageWorkflow.indexOf('- name: Build NSIS installer');
  const packageVerify = packageWorkflow.indexOf('- name: Extract and verify NSIS installer');
  const packageUpload = packageWorkflow.indexOf('- name: Upload verified Windows package');
  assert.ok(packageStageVerify >= 0 && packageStageVerify < packageBuild);
  assert.ok(packageBuild < packageVerify && packageVerify < packageUpload);

  assert.match(releaseWorkflow, /scripts\/build-windows-installer\.ps1/);
  assert.match(releaseWorkflow, /scripts\/verify-windows-installer\.ps1/);
  assert.match(releaseWorkflow, /dist\/windows-package-validation\.json/);
  assert.doesNotMatch(releaseWorkflow, /function Resolve-MakeNsis/);
  const releaseStageVerify = releaseWorkflow.indexOf('- name: Verify staged Windows payload');
  const releaseBuild = releaseWorkflow.indexOf('- name: Build NSIS installer (Windows)');
  const releaseVerify = releaseWorkflow.indexOf('- name: Extract and verify NSIS installer (Windows)');
  const releaseUpload = releaseWorkflow.indexOf('- name: Upload bundles to release');
  assert.ok(releaseStageVerify >= 0 && releaseStageVerify < releaseBuild);
  assert.ok(releaseBuild < releaseVerify);
  assert.ok(releaseVerify < releaseUpload);

  assert.match(buildScript, /function Resolve-MakeNsis/);
  assert.match(buildScript, /C:\\Program Files \(x86\)\\NSIS\\makensis\.exe/);
  assert.match(buildScript, /choco\.exe/);
  assert.match(buildScript, /downloads\.sourceforge\.net\/project\/nsis/);
  assert.match(buildScript, /Mini-Term_\$\(\$versionInfo\.Semantic\)_x64-setup\.exe/);
  assert.match(buildScript, /Numeric = '\{0\}\.\{1\}\.\{2\}\.0'/);

  for (const payload of [
    'mini-term.exe',
    'miniterm-hook.exe',
    'mt-ssh-cli.exe',
    'mt-ssh-mcp.exe',
    'mt-terminal-host.exe',
    'portable-conpty/conpty.dll',
    'portable-conpty/x64/OpenConsole.exe',
    'portable-conpty/arm64/OpenConsole.exe',
  ]) {
    assert.ok(verifyScript.includes(payload), `${payload} must be package-verified`);
  }
  assert.match(verifyScript, /Get-FileHash .* -Algorithm SHA256/);
  assert.match(verifyScript, /staged_sha256 = \$stagedHash/);
  assert.match(verifyScript, /extracted_sha256 = \$extractedHash/);
  assert.match(verifyScript, /required PE resource type/);
  assert.match(verifyScript, /windows-package-validation\.json/);
  for (const marker of [
    'MINI_TERM_LEGACY_SHELL',
    'MINI_TERM_TERMINAL_HOST',
    'MINI_TERM_REMOTE_RUNTIME',
    'MINI_TERM_REMOTE_AGENT_STATUS',
    'MINI_TERM_ORCA_WORKTREE_CONTEXT',
    'MINI_TERM_GITHUB_PROJECT_TASKS',
    'MINI_TERM_GLOBAL_AGENT_ACTIVITY',
  ]) {
    assert.ok(verifyScript.includes(marker), `${marker} must be package-verified`);
  }

  for (const retiredPath of [
    'docker-compose.ci.yml',
    'scripts/docker-ci.sh',
    'docker/ci/Dockerfile',
    'docker/ci/README.md',
  ]) {
    assert.equal(await pathExists(join(repoRoot, retiredPath)), false, `${retiredPath} must be absent`);
  }
  const actionsSources = [ciWorkflow, releaseWorkflow, packageWorkflow, buildScript, verifyScript]
    .join('\n');
  assert.doesNotMatch(
    actionsSources,
    /docker-compose\.ci\.yml|scripts\/docker-ci\.sh|docker\/ci\//,
  );
});
