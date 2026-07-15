import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

import { parse } from 'yaml';

function isMapping(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function requireMapping(value, path) {
  if (!isMapping(value)) {
    throw new Error(`${path} must be a mapping`);
  }
  return value;
}

function requireExactKeys(value, expected, path) {
  const mapping = requireMapping(value, path);
  const actual = Object.keys(mapping).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    throw new Error(`${path} must contain only: ${wanted.join(', ')}`);
  }
  return mapping;
}

function requireStep(steps, predicate, description) {
  const matches = steps.filter(predicate);
  if (matches.length !== 1) {
    throw new Error(`The release job must contain exactly one ${description} step`);
  }
  return matches[0];
}

export function validateReleaseWorkflow(source) {
  const workflow = requireMapping(parse(source), 'workflow');

  const triggers = requireExactKeys(workflow.on, ['push'], 'on');
  const push = requireExactKeys(triggers.push, ['tags'], 'on.push');
  if (!Array.isArray(push.tags) || push.tags.length !== 1 || push.tags[0] !== 'v*') {
    throw new Error("on.push.tags must be exactly ['v*']");
  }

  const permissions = requireExactKeys(workflow.permissions, ['contents'], 'permissions');
  if (permissions.contents !== 'write') {
    throw new Error('permissions.contents must be write');
  }

  const jobs = requireMapping(workflow.jobs, 'jobs');
  const jobNames = Object.keys(jobs);
  if (jobNames.length !== 1) {
    throw new Error('jobs must contain exactly one release job');
  }

  const job = requireMapping(jobs[jobNames[0]], `jobs.${jobNames[0]}`);
  if (job['runs-on'] !== 'windows-latest') {
    throw new Error('the release job must run on windows-latest');
  }
  if (Object.hasOwn(job, 'permissions')) {
    throw new Error('the release job must not override top-level permissions');
  }
  if (!Array.isArray(job.steps)) {
    throw new Error('the release job steps must be a sequence');
  }

  const steps = job.steps.map((step, index) => requireMapping(step, `steps[${index}]`));
  requireStep(steps, (step) => step.uses === 'actions/checkout@v4', 'checkout');

  const nodeStep = requireStep(
    steps,
    (step) => step.uses === 'actions/setup-node@v4',
    'Node setup',
  );
  const nodeInputs = requireMapping(nodeStep.with, 'Node setup inputs');
  if (String(nodeInputs['node-version']) !== '22') {
    throw new Error('Node setup must use Node 22');
  }

  requireStep(
    steps,
    (step) => step.uses === 'dtolnay/rust-toolchain@stable',
    'stable Rust setup',
  );

  for (const command of [
    'npm ci',
    'npm run test:release-workflow',
    'npm test -- --run',
    'npm run rust:test',
  ]) {
    requireStep(steps, (step) => step.run === command, `${command} command`);
  }

  const publishStep = requireStep(
    steps,
    (step) => step.uses === 'tauri-apps/tauri-action@v1',
    'Tauri publish',
  );
  const environment = requireExactKeys(publishStep.env, ['GITHUB_TOKEN'], 'Tauri publish env');
  if (environment.GITHUB_TOKEN !== '${{ secrets.GITHUB_TOKEN }}') {
    throw new Error('Tauri publish GITHUB_TOKEN must use secrets.GITHUB_TOKEN');
  }

  const inputs = requireExactKeys(
    publishStep.with,
    ['tagName', 'releaseName', 'releaseDraft', 'prerelease', 'args'],
    'Tauri publish inputs',
  );
  const expectedInputs = {
    tagName: 'v__VERSION__',
    releaseName: 'Gpt Orbit v__VERSION__',
    releaseDraft: true,
    prerelease: false,
    args: '--config src-tauri/tauri.weekly.conf.json',
  };
  for (const [name, expected] of Object.entries(expectedInputs)) {
    if (inputs[name] !== expected) {
      throw new Error(`Tauri publish ${name} must equal ${String(expected)}`);
    }
  }
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : undefined;
if (invokedPath === fileURLToPath(import.meta.url)) {
  try {
    const workflowPath = process.argv[2];
    if (!workflowPath) {
      throw new Error('workflow path argument is required');
    }
    validateReleaseWorkflow(readFileSync(workflowPath, 'utf8'));
  } catch (error) {
    console.error(`Release workflow contract failed: ${error.message}`);
    process.exitCode = 1;
  }
}
