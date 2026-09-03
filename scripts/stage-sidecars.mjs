// 构建 mini-term 的辅助二进制并就位。
//
// 三个 agent/SSH sidecar 在独立工作区 sidecars/；mt-terminal-host 跟随主程序
// 版本，住在根 workspace。运行时都按 current_exe().parent() 同目录裸名定位，
// 拷进主程序所在的 target/<profile>/，dev 与 release 同一套就位模型；
// Windows 下再把便携 ConPTY 一并就位到同目录的 portable-conpty/。
//
//   node scripts/stage-sidecars.mjs                       dev: debug，triple 取 rustc host
//   node scripts/stage-sidecars.mjs --release --target T  CI:  release，triple = T
//   node scripts/stage-sidecars.mjs --verify-only --release --target T
//                                                        校验完整发布 staging
//
// CARGO_TARGET_DIR controls the writable build root. MINI_TERM_STAGE_DIR, when set,
// is the exact directory that receives runnable artifacts; otherwise the root Cargo
// target's profile directory remains the destination.
//
// release.yml 的便携包组装步骤直接从 target/release/ 收集 exe、sidecar 与
// portable-conpty/ —— 目录布局即最终 zip 布局。

import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { lstat, rm } from 'node:fs/promises';
import { isAbsolute, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import {
  readPeMachine,
  stagePortableConpty,
  validatePortableConptyLayout,
  WINDOWS_X64_TARGET,
} from './stage-conpty.mjs';

const SIDECARS = ['miniterm-hook', 'mt-ssh-mcp', 'mt-ssh-cli'];
const MANIFEST = join('sidecars', 'Cargo.toml');
const ROOT_HELPERS = ['mt-terminal-host'];
const APP_BINARY = 'mini-term';
const BUILD_NAMESPACE = '.stage-sidecars';
const WINDOWS_X64_PE_MACHINE = 0x8664;

function absolutePath(cwd, value) {
  return isAbsolute(value) ? value : resolve(cwd, value);
}

function targetFromArgs(args) {
  const targetIndex = args.indexOf('--target');
  if (targetIndex === -1) return null;

  const target = args[targetIndex + 1];
  if (!target || target.startsWith('--')) {
    throw new Error('[stage-sidecars] --target requires a target triple');
  }
  return target;
}

// CI 用显式 --target；dev 取 rustc 自报的 host triple（按 platform/arch 手拼易错）。
function hostTriple() {
  const out = execFileSync('rustc', ['-Vv'], { encoding: 'utf8' });
  const line = out.split('\n').find((l) => l.startsWith('host:'));
  if (!line) throw new Error('无法从 rustc -Vv 解析 host triple');
  return line.slice('host:'.length).trim();
}

function artifactDir(targetDir, explicitTarget, profile) {
  return explicitTarget ? join(targetDir, explicitTarget, profile) : join(targetDir, profile);
}

function assertSupportedStagingTarget(target) {
  if (target.includes('windows') && target !== WINDOWS_X64_TARGET) {
    throw new Error(
      `[stage-sidecars] Windows 目标 ${target} 尚无匹配的 x64 sidecar/便携 ConPTY 资源`,
    );
  }
}

export function createStagingPlan({
  args = [],
  cwd = process.cwd(),
  env = process.env,
  triple,
} = {}) {
  const release = args.includes('--release');
  const explicitTarget = targetFromArgs(args);
  const resolvedTriple = explicitTarget ?? triple;
  if (!resolvedTriple) {
    throw new Error('[stage-sidecars] target triple is required for path planning');
  }
  assertSupportedStagingTarget(resolvedTriple);

  const profile = release ? 'release' : 'debug';
  const extension = resolvedTriple.includes('windows') ? '.exe' : '';
  const rootTargetDir = absolutePath(cwd, env.CARGO_TARGET_DIR ?? 'target');
  // Keep both independently locked workspaces away from the runnable destination.
  // In particular, Windows cannot relink a running mt-terminal-host.exe in place.
  const sidecarTargetDir = env.CARGO_TARGET_DIR
    ? join(rootTargetDir, BUILD_NAMESPACE, 'sidecars')
    : resolve(cwd, 'sidecars', 'target');
  const rootHelperTargetDir = join(rootTargetDir, BUILD_NAMESPACE, 'root-helper');
  const stageDir = env.MINI_TERM_STAGE_DIR
    ? absolutePath(cwd, env.MINI_TERM_STAGE_DIR)
    : join(rootTargetDir, profile);
  const sidecarBuiltDir = artifactDir(sidecarTargetDir, explicitTarget, profile);
  const rootHelperBuiltDir = artifactDir(rootHelperTargetDir, explicitTarget, profile);
  const conptyCacheDir = env.CARGO_TARGET_DIR
    ? join(rootTargetDir, BUILD_NAMESPACE, 'conpty-cache')
    : env.MINI_TERM_STAGE_DIR
      ? join(stageDir, '.conpty-cache')
      : resolve(cwd, '.conpty-cache');

  const sidecarCargoArgs = ['build', '--locked', '--manifest-path', MANIFEST];
  const rootCargoArgs = ['build', '--locked'];
  if (release) {
    sidecarCargoArgs.push('--release');
    rootCargoArgs.push('--release');
  }
  if (explicitTarget) {
    sidecarCargoArgs.push('--target', explicitTarget);
    rootCargoArgs.push('--target', explicitTarget);
  }
  for (const name of SIDECARS) sidecarCargoArgs.push('--bin', name);
  for (const name of ROOT_HELPERS) rootCargoArgs.push('--bin', name);

  return {
    cwd,
    release,
    explicitTarget,
    triple: resolvedTriple,
    profile,
    extension,
    stageDir,
    sidecarTargetDir,
    rootHelperTargetDir,
    sidecarBuiltDir,
    rootHelperBuiltDir,
    conptyCacheDir,
    sidecarCargoArgs,
    rootCargoArgs,
  };
}

function builtArtifacts(plan) {
  return [
    ...SIDECARS.map((name) => ({
      name,
      path: join(plan.sidecarBuiltDir, `${name}${plan.extension}`),
    })),
    ...ROOT_HELPERS.map((name) => ({
      name,
      path: join(plan.rootHelperBuiltDir, `${name}${plan.extension}`),
    })),
  ];
}

function stagedArtifacts(plan, { includeApp = false } = {}) {
  const names = includeApp ? [...SIDECARS, ...ROOT_HELPERS, APP_BINARY] : [...SIDECARS, ...ROOT_HELPERS];
  return names.map((name) => ({
    name,
    path: join(plan.stageDir, `${name}${plan.extension}`),
  }));
}

async function validateArtifacts(artifacts, triple) {
  const expectedMachine = triple === WINDOWS_X64_TARGET ? WINDOWS_X64_PE_MACHINE : null;
  for (const artifact of artifacts) {
    let info;
    try {
      info = await lstat(artifact.path);
    } catch (error) {
      throw new Error(
        `[stage-sidecars] 缺少 ${artifact.name} 构建产物 ${artifact.path}: ${copyErrorDetail(error)}`,
      );
    }
    if (!info.isFile() || info.size === 0) {
      throw new Error(`[stage-sidecars] ${artifact.name} 不是非空普通文件: ${artifact.path}`);
    }
    if (expectedMachine !== null) {
      const machine = await readPeMachine(artifact.path);
      if (machine !== expectedMachine) {
        throw new Error(
          `[stage-sidecars] ${artifact.name} PE machine 不匹配: expected=0x${expectedMachine.toString(16)} actual=0x${machine.toString(16)}`,
        );
      }
    }
  }
  return artifacts;
}

export async function validateBuildArtifacts(plan) {
  return validateArtifacts(builtArtifacts(plan), plan.triple);
}

export async function validateStagedPayload(
  plan,
  { includeApp = false, verifyOfficialConptyHashes = false } = {},
) {
  const artifacts = await validateArtifacts(stagedArtifacts(plan, { includeApp }), plan.triple);
  if (plan.triple === WINDOWS_X64_TARGET) {
    await validatePortableConptyLayout(join(plan.stageDir, 'portable-conpty'), {
      verifyOfficialHashes: verifyOfficialConptyHashes,
    });
  }
  return artifacts;
}

function copyErrorDetail(error) {
  if (error && typeof error === 'object') return error.code ?? error.message ?? String(error);
  return String(error);
}

export function copyStagedArtifact({
  from,
  staged,
  release,
  copyFile = copyFileSync,
  log = console.log,
  warn = console.warn,
}) {
  try {
    copyFile(from, staged);
    log(`[stage-sidecars] ${from} -> ${staged}`);
    return true;
  } catch (error) {
    if (release) throw error;
    warn(
      `[stage-sidecars] 跳过 ${staged}（可能正在运行）: ${copyErrorDetail(error)}`,
    );
    return false;
  }
}

async function cleanupFailedReleaseStage(plan, warn = console.warn) {
  const paths = stagedArtifacts(plan).map((artifact) => artifact.path);
  paths.push(join(plan.stageDir, 'portable-conpty'));
  const results = await Promise.allSettled(
    paths.map((path) => rm(path, { recursive: true, force: true })),
  );
  for (const [index, result] of results.entries()) {
    if (result.status === 'rejected') {
      warn(
        `[stage-sidecars] 清理失败 ${paths[index]}: ${copyErrorDetail(result.reason)}`,
      );
    }
  }
}

export async function stageSidecars({
  args = process.argv.slice(2),
  cwd = process.cwd(),
  env = process.env,
  execFile = execFileSync,
  stageConpty = stagePortableConpty,
  log = console.log,
  warn = console.warn,
} = {}) {
  const explicitTarget = targetFromArgs(args);
  const plan = createStagingPlan({
    args,
    cwd,
    env,
    triple: explicitTarget ?? hostTriple(),
  });

  if (args.includes('--verify-only')) {
    await validateStagedPayload(plan, {
      includeApp: true,
      verifyOfficialConptyHashes: plan.release,
    });
    log(`[stage-sidecars] verified ${plan.stageDir}`);
    return plan;
  }

  try {
    log(`[stage-sidecars] triple=${plan.triple} profile=${plan.profile}`);
    log(`[stage-sidecars] cargo ${plan.sidecarCargoArgs.join(' ')}`);
    execFile('cargo', plan.sidecarCargoArgs, {
      cwd: plan.cwd,
      env: { ...env, CARGO_TARGET_DIR: plan.sidecarTargetDir },
      stdio: 'inherit',
    });

    log(`[stage-sidecars] cargo ${plan.rootCargoArgs.join(' ')}`);
    execFile('cargo', plan.rootCargoArgs, {
      cwd: plan.cwd,
      env: { ...env, CARGO_TARGET_DIR: plan.rootHelperTargetDir },
      stdio: 'inherit',
    });

    const artifacts = await validateBuildArtifacts(plan);
    mkdirSync(plan.stageDir, { recursive: true });
    for (const artifact of artifacts) {
      copyStagedArtifact({
        from: artifact.path,
        staged: join(plan.stageDir, `${artifact.name}${plan.extension}`),
        release: plan.release,
        log,
        warn,
      });
    }

    if (plan.triple === WINDOWS_X64_TARGET) {
      await stageConpty({
        target: plan.triple,
        cacheDir: plan.conptyCacheDir,
        outputDir: join(plan.stageDir, 'portable-conpty'),
      });
    } else {
      if (plan.release) {
        await rm(join(plan.stageDir, 'portable-conpty'), { recursive: true, force: true });
      }
      log(`[stage-sidecars] ${plan.triple} 非 Windows，跳过便携 ConPTY staging`);
    }

    await validateStagedPayload(plan, {
      verifyOfficialConptyHashes: plan.release,
    });
    log('[stage-sidecars] done');
    return plan;
  } catch (error) {
    if (plan.release) {
      await cleanupFailedReleaseStage(plan, warn);
    }
    throw error;
  }
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : null;
if (invokedPath === import.meta.url) {
  await stageSidecars();
}
