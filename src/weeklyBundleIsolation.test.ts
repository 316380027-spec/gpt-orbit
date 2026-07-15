// @vitest-environment node

import { mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { build } from 'vite';

const STANDARD_ONLY_COPY = [
  '5H LEFT',
  'Codex · 5 小时',
  'Codex 5 小时额度',
] as const;

async function emittedText(root: string): Promise<string> {
  const entries = await readdir(root, { withFileTypes: true });
  const chunks = await Promise.all(
    entries.map(async (entry) => {
      const path = join(root, entry.name);
      return entry.isDirectory() ? emittedText(path) : readFile(path, 'utf8');
    }),
  );
  return chunks.join('\n');
}

describe('production bundle isolation', () => {
  let outputRoot = '';
  let standardOutput = '';
  let weeklyOutput = '';

  beforeAll(async () => {
    outputRoot = await mkdtemp(join(tmpdir(), 'gpt-orbit-bundles-'));
    const standardDir = join(outputRoot, 'standard');
    const weeklyDir = join(outputRoot, 'weekly');

    await build({
      mode: 'production',
      build: { emptyOutDir: true, outDir: standardDir },
    });
    await build({
      mode: 'weekly',
      build: { emptyOutDir: true, outDir: weeklyDir },
    });

    standardOutput = await emittedText(standardDir);
    weeklyOutput = await emittedText(weeklyDir);
  }, 30_000);

  afterAll(async () => {
    if (outputRoot !== '') {
      await rm(outputRoot, { force: true, recursive: true });
    }
  });

  it.each(STANDARD_ONLY_COPY)('keeps %s in the standard bundle', (copy) => {
    expect(standardOutput).toContain(copy);
  });

  it.each(STANDARD_ONLY_COPY)('excludes %s from the weekly bundle', (copy) => {
    expect(weeklyOutput).not.toContain(copy);
  });

  it('anchors the native-size weekly visual inside the compact canvas', async () => {
    const css = await readFile(
      new URL('./features/orbit/orbit-widget.css', import.meta.url),
      'utf8',
    );
    const rule = css.match(/\.weekly-orbit-widget\s*\{[^}]+\}/)?.[0] ?? '';

    expect(rule).toMatch(/position:\s*absolute/);
    expect(rule).toMatch(/left:\s*6px/);
    expect(rule).toMatch(/top:\s*6px/);
    expect(rule).toMatch(/width:\s*92px/);
    expect(rule).toMatch(/height:\s*74px/);
    expect(rule).not.toMatch(/transform:\s*scale/);
  });
});
