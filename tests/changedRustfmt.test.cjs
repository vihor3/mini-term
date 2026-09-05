const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');

const script = path.resolve(__dirname, '../.github/scripts/check_changed_rustfmt.mjs');
const rustfmtArgs = ['--edition', '2024', '--config', 'skip_children=true'];

function run(cwd, command, args, env = process.env) {
  const result = spawnSync(command, args, {
    cwd,
    env,
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024,
    timeout: 30_000,
  });
  if (result.error) throw result.error;
  return result;
}

function succeed(cwd, command, args) {
  const result = run(cwd, command, args);
  assert.equal(result.status, 0, `${command}: ${result.stdout}\n${result.stderr}`);
  return result.stdout;
}

// All formatter execution and Git mutations stay in disposable Actions fixtures.
function fixture(t, baseline, changes) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mini-term-rustfmt-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const repo = path.join(root, 'repo');
  fs.mkdirSync(repo);
  succeed(repo, 'git', ['init', '--quiet']);
  succeed(repo, 'git', ['config', 'user.name', 'Rustfmt fixtures']);
  succeed(repo, 'git', ['config', 'user.email', 'rustfmt-fixtures@example.invalid']);
  succeed(repo, 'git', ['config', 'commit.gpgSign', 'false']);
  succeed(repo, 'git', ['config', 'core.autocrlf', 'false']);
  succeed(repo, 'git', ['config', 'core.hooksPath', path.join(root, 'no-hooks')]);

  for (const [message, sources] of [['baseline', baseline], ['changes', changes]]) {
    for (const [file, source] of Object.entries(sources)) {
      const target = path.join(repo, file);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, source);
    }
    succeed(repo, 'git', ['add', '--', '.']);
    succeed(repo, 'git', ['commit', '--quiet', '-m', message]);
  }

  return {
    root,
    repo,
    base: succeed(repo, 'git', ['rev-parse', 'HEAD^']).trim(),
    sources: { ...baseline, ...changes },
    patchPath: path.join(root, 'changed-rustfmt.patch'),
    fullPatchPath: path.join(root, 'full-rustfmt.patch'),
  };
}

function check(f) {
  return run(f.repo, process.execPath, [script, f.base], {
    ...process.env,
    RUSTFMT_PATCH_PATH: f.patchPath,
    RUSTFMT_FULL_PATCH_PATH: f.fullPatchPath,
  });
}

function assertCompletePatches(f, affectedFiles) {
  const expected = { ...f.sources };
  for (const [index, file] of affectedFiles.entries()) {
    const formattedPath = path.join(f.root, `expected-${index}.rs`);
    fs.writeFileSync(formattedPath, f.sources[file]);
    succeed(f.repo, 'rustfmt', [...rustfmtArgs, formattedPath]);
    expected[file] = fs.readFileSync(formattedPath, 'utf8');
    assert.notEqual(expected[file], f.sources[file], `${file} must need formatting`);
  }

  const patch = fs.readFileSync(f.patchPath, 'utf8');
  assert.equal(patch, fs.readFileSync(f.fullPatchPath, 'utf8'));
  const patchedFiles = [...patch.matchAll(/^\+\+\+ b\/(.+)$/gm)].map((match) => match[1]);
  assert.deepEqual(patchedFiles.sort(), [...affectedFiles].sort());
  for (const [file, source] of Object.entries(f.sources)) {
    assert.equal(fs.readFileSync(path.join(f.repo, file), 'utf8'), source);
  }

  for (const patchPath of [f.patchPath, f.fullPatchPath]) {
    // No --unidiff-zero or offset relaxation: both public artifacts must apply
    // normally and reproduce complete rustfmt output, including baseline hunks.
    succeed(f.repo, 'git', ['apply', patchPath]);
    for (const [file, source] of Object.entries(expected)) {
      assert.equal(fs.readFileSync(path.join(f.repo, file), 'utf8'), source, file);
    }
    succeed(f.repo, 'git', ['apply', '--reverse', patchPath]);
  }
  return { patch, expected };
}

const importBaseline = `use super::identity::TerminalRoute;
use super::pure::{
    resolve_auto_resume_command, resolve_resume_cwd, resolve_scrollback,
    terminal_style_from,
};

fn baseline_formatting() -> u8 { 7 }
`;
const movedImport = 'use super::{AppStore, ProjectState, TerminalJumpTarget};';
const importChanges = `${movedImport}\n${importBaseline}`;

const declarationBaseline = `fn baseline_formatting() -> u8 { 7 }
`;
const declarationChanges = `${declarationBaseline}
#[cfg(test)]
mod tests {
    #[test]
    fn aliases_preserve_states() {
        let mut request = close_request();
        let state = source_state();
        let alias_state = alias_state();
        let alias_binding = request.binding.clone();
        let bindings = HashMap::from([
            ("owner".into(), request.binding.clone()), ("alias".into(), alias_binding),
        ]);
        let mut states = HashMap::from([
            ("owner".into(), state), ("alias".into(), alias_state),
        ]);
        request.aliases = terminal_close_aliases(&request.target, &bindings, &states).unwrap();
        assert!(request.may_replace_alias_layout(None));
    }
}
`;

const baselineOnly = `fn baseline_formatting() -> u8 { 3 }

// Keep the source edit separate from the baseline formatting.
// This line is unchanged.
// This line is also unchanged.
// Only the following comment changes.
// revision: before
`;
const baselineOnlyChanges = baselineOnly.replace('revision: before', 'revision: after');

test('moved imports retain the baseline-side insertion in both complete patches', (t) => {
  const f = fixture(t, { 'imports.rs': importBaseline }, { 'imports.rs': importChanges });
  const result = check(f);
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stdout, /baseline ignored outside changed lines: [1-9]\d*;/);
  assert.match(result.stdout, /changed-line formatting hunks: 1\b/);
  const { patch, expected } = assertCompletePatches(f, ['imports.rs']);
  assert.ok(patch.includes(`-${movedImport}\n`));
  assert.ok(patch.includes(`+${movedImport}\n`));
  assert.ok(expected['imports.rs'].includes(`};\n${movedImport}\n`));
  assert.ok(expected['imports.rs'].includes('fn baseline_formatting() -> u8 {\n    7\n}'));
});

test('split declaration hunks keep states before its use despite ignored earlier offsets', (t) => {
  const f = fixture(
    t,
    { 'lifecycle.rs': declarationBaseline },
    { 'lifecycle.rs': declarationChanges },
  );
  const result = check(f);
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stdout, /baseline ignored outside changed lines: [1-9]\d*;/);
  assert.match(result.stderr, /\n-        let mut states = HashMap::from\(\[/);
  assert.match(result.stderr, /@@ -\d+,0 \+\d+ @@\n\+        let mut states = HashMap::from/);
  const { expected } = assertCompletePatches(f, ['lifecycle.rs']);
  assert.ok(expected['lifecycle.rs'].includes(
    '        let mut states = HashMap::from([("owner".into(), state), ("alias".into(), alias_state)]);\n'
      + '        request.aliases = terminal_close_aliases(&request.target, &bindings, &states).unwrap();',
  ));
});

test('baseline-only formatting stays non-failing and produces no patch', (t) => {
  const f = fixture(t, {
    'baseline.rs': baselineOnly,
    'untouched.rs': 'fn untouched() -> u8 { 9 }\n',
    'formatted.rs': 'const REVISION: u8 = 1;\n',
  }, {
    'baseline.rs': baselineOnlyChanges,
    'formatted.rs': 'const REVISION: u8 = 2;\n',
  });
  const result = check(f);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /baseline ignored outside changed lines: 1; changed-line formatting hunks: 0\b/);
  assert.equal(fs.existsSync(f.patchPath), false);
  assert.equal(fs.existsSync(f.fullPatchPath), false);
  for (const [file, source] of Object.entries(f.sources)) {
    assert.equal(fs.readFileSync(path.join(f.repo, file), 'utf8'), source);
  }
});

test('multiple affected files are complete while baseline-only and untouched files stay excluded', (t) => {
  const f = fixture(t, {
    'src/imports.rs': importBaseline,
    'src/lifecycle.rs': declarationBaseline,
    'src/baseline.rs': baselineOnly,
    'src/untouched.rs': 'fn untouched() -> u8 { 9 }\n',
  }, {
    'src/imports.rs': importChanges,
    'src/lifecycle.rs': declarationChanges,
    'src/baseline.rs': baselineOnlyChanges,
  });
  const result = check(f);
  assert.equal(result.status, 1, result.stderr);
  assert.doesNotMatch(result.stderr, /Formatting differs in src\/(?:baseline|untouched)\.rs/);
  assertCompletePatches(f, ['src/imports.rs', 'src/lifecycle.rs']);
});
