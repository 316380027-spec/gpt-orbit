import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { validateReleaseWorkflow } from './validate-release-workflow.mjs';

const workflowPath = new URL('../../.github/workflows/release.yml', import.meta.url);
const validWorkflow = (await readFile(workflowPath, 'utf8')).replaceAll('\r\n', '\n');

function replace(source, expected, replacement) {
  assert.ok(source.includes(expected), `Fixture did not contain: ${expected}`);
  return source.replace(expected, replacement);
}

function rejects(name, source) {
  test(name, () => {
    assert.throws(() => validateReleaseWorkflow(source));
  });
}

test('accepts the checked-in release workflow', () => {
  assert.doesNotThrow(() => validateReleaseWorkflow(validWorkflow));
});

test('requires the manual v0.1.0 release guard', () => {
  const withoutGuard = validWorkflow.replace("        if: github.ref_name != 'v0.1.0'\n", '');
  assert.notEqual(withoutGuard, validWorkflow, 'Fixture must contain the v0.1.0 guard');
  assert.throws(() => validateReleaseWorkflow(withoutGuard));
});

rejects(
  'rejects required text that exists only in comments or the wrong job',
  `# tags: 'v*' contents: write windows-latest npm ci npm test -- --run
# npm run test:release-workflow npm run rust:test tauri-apps/tauri-action@v1
# GITHUB_TOKEN: \${{ secrets.GITHUB_TOKEN }}
# tagName: v__VERSION__ releaseName: Gpt Orbit v__VERSION__
# releaseDraft: true prerelease: false --config src-tauri/tauri.weekly.conf.json
name: Invalid release
on:
  push:
    branches: [main]
permissions:
  contents: read
jobs:
  wrong-job:
    runs-on: ubuntu-latest
    steps:
      - uses: tauri-apps/tauri-action@v1
`,
);

for (const [name, source] of [
  [
    'rejects a missing push tags structure',
    replace(validWorkflow, "    tags:\n      - 'v*'", '    branches:\n      - main'),
  ],
  [
    'rejects a tag pattern other than v*',
    replace(validWorkflow, "      - 'v*'", "      - 'release-*'"),
  ],
  [
    'rejects more than one job',
    `${validWorkflow}\n  extra-job:\n    runs-on: windows-latest\n    steps: []\n`,
  ],
  [
    'rejects a non-Windows runner',
    replace(validWorkflow, 'runs-on: windows-latest', 'runs-on: ubuntu-latest'),
  ],
  [
    'rejects permissions beyond only contents write',
    replace(validWorkflow, '  contents: write', '  contents: write\n  actions: read'),
  ],
  [
    'rejects job-level permissions that override the top-level permissions',
    replace(
      validWorkflow,
      '  release-windows:\n    runs-on: windows-latest',
      `  release-windows:
    permissions:
      contents: write
      actions: write
    runs-on: windows-latest`,
    ),
  ],
  [
    'rejects Node other than version 22',
    replace(validWorkflow, 'node-version: 22', 'node-version: 20'),
  ],
  [
    'rejects Rust other than stable',
    replace(validWorkflow, 'dtolnay/rust-toolchain@stable', 'dtolnay/rust-toolchain@nightly'),
  ],
  [
    'rejects a missing checkout step',
    replace(validWorkflow, 'uses: actions/checkout@v4', 'uses: actions/cache@v4'),
  ],
  [
    'rejects a missing npm ci command',
    replace(validWorkflow, 'run: npm ci', 'run: npm install'),
  ],
  [
    'rejects a missing release workflow contract test command',
    replace(
      validWorkflow,
      'run: npm run test:release-workflow',
      'run: node scripts/release/validate-release-workflow.node.mjs',
    ),
  ],
  [
    'rejects a missing frontend test command',
    replace(validWorkflow, 'run: npm test -- --run', 'run: npm test'),
  ],
  [
    'rejects a missing default Rust test command',
    replace(validWorkflow, 'run: npm run rust:test', 'run: npm run rust:test -- --features weekly'),
  ],
  [
    'rejects a different Tauri action major',
    replace(validWorkflow, 'tauri-apps/tauri-action@v1', 'tauri-apps/tauri-action@v0'),
  ],
  [
    'rejects an incorrect GITHUB_TOKEN expression',
    replace(
      validWorkflow,
      'GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}',
      'GITHUB_TOKEN: ${{ github.token }}',
    ),
  ],
  [
    'rejects an incorrect tagName',
    replace(validWorkflow, 'tagName: v__VERSION__', 'tagName: weekly-v__VERSION__'),
  ],
  [
    'rejects an incorrect releaseName',
    replace(validWorkflow, 'releaseName: Gpt Orbit v__VERSION__', 'releaseName: v__VERSION__'),
  ],
  [
    'rejects releaseDraft false',
    replace(validWorkflow, 'releaseDraft: true', 'releaseDraft: false'),
  ],
  [
    'rejects prerelease true',
    replace(validWorkflow, 'prerelease: false', 'prerelease: true'),
  ],
  [
    'rejects incorrect Weekly config args',
    replace(
      validWorkflow,
      'args: --config src-tauri/tauri.weekly.conf.json',
      'args: --config src-tauri/tauri.conf.json',
    ),
  ],
]) {
  rejects(name, source);
}
