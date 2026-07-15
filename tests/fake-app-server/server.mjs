#!/usr/bin/env node
import { createInterface } from 'node:readline';
import { existsSync, mkdirSync, openSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const scenario = process.env.GPT_ORBIT_FAKE_SCENARIO ?? 'live';
const stateDir = process.env.GPT_ORBIT_FAKE_STATE_DIR ?? process.cwd();
const supported = new Set([
  'live',
  'sparse-weekly',
  'weekly-missing',
  'login-required',
  'disconnect-once',
  'malformed-then-valid',
]);

if (!supported.has(scenario)) {
  process.stderr.write('unsupported fake scenario\n');
  process.exit(3);
}

if (process.argv.slice(2).join(' ') !== 'app-server --listen stdio://') {
  process.stderr.write('invalid fake app-server invocation\n');
  process.exit(2);
}

mkdirSync(stateDir, { recursive: true });
writeFileSync(join(stateDir, 'last-scenario'), scenario);
recordChildStart();
process.on('exit', () => {
  try {
    rmSync(join(stateDir, 'active-child'));
  } catch {
    // best-effort fixture cleanup
  }
});

if (scenario === 'disconnect-once') {
  const marker = join(stateDir, 'disconnect-once-marker');
  try {
    openSync(marker, 'wx');
    process.stderr.write('controlled one-time disconnect\n');
    process.exit(0);
  } catch {
    // Marker already exists: serve normally on restart.
  }
}

let initialized = false;
let loginCompleted = scenario !== 'login-required';
let malformedSent = false;
let sparseSent = false;

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on('line', (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  const { id, method } = message;

  if (method === 'initialize') {
    if (scenario === 'malformed-then-valid' && !malformedSent) {
      process.stdout.write('{malformed-json\n');
      malformedSent = true;
    }
    respond({ id, result: { userAgent: 'gpt-orbit-fake', codexHome: 'isolated', platformFamily: 'windows', platformOs: 'windows' } });
    return;
  }

  if (method === 'initialized') {
    initialized = true;
    return;
  }

  if (!initialized) {
    respond({ id, error: { code: -32002, message: 'Not initialized' } });
    return;
  }

  if (method === 'account/read') {
    respond({
      id,
      result: {
        account: loginCompleted
          ? { type: 'chatgpt', planType: 'plus', email: 'fixture@example.invalid' }
          : null,
      },
    });
    return;
  }

  if (method === 'account/login/start') {
    loginCompleted = true;
    respond({ id, result: { loginId: 'fake-login', authUrl: 'https://example.invalid/gpt-orbit-login' } });
    notify({ method: 'account/login/completed', params: { loginId: 'fake-login', success: true, error: null } });
    return;
  }

  if (method === 'account/rateLimits/read') {
    const rateLimits = {
      primary: { usedPercent: 27, windowDurationMins: 300, resetsAt: 1800000000 },
      planType: 'plus',
    };
    if (scenario !== 'weekly-missing') {
      rateLimits.secondary = { usedPercent: 42, windowDurationMins: 10080, resetsAt: 1800500000 };
    }
    respond({ id, result: { rateLimits } });
    if (scenario === 'sparse-weekly' && !sparseSent) {
      sparseSent = true;
      notify({ method: 'account/rateLimits/updated', params: { rateLimits: { primary: { usedPercent: 55 } } } });
    }
    return;
  }

  respond({ id, error: { code: -32601, message: 'Method not found' } });
});

function respond(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function notify(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function recordChildStart() {
  const spawnCountPath = join(stateDir, 'spawn-count');
  const spawnCount = readNumber(spawnCountPath) + 1;
  writeFileSync(spawnCountPath, String(spawnCount));

  const activePath = join(stateDir, 'active-child');
  const maxPath = join(stateDir, 'max-simultaneous-children');
  if (existsSync(activePath)) {
    writeFileSync(maxPath, '2');
    return;
  }
  writeFileSync(activePath, String(process.pid));
  if (!existsSync(maxPath)) {
    writeFileSync(maxPath, '1');
  }
}

function readNumber(path) {
  try {
    return Number.parseInt(readFileSync(path, 'utf8'), 10) || 0;
  } catch {
    return 0;
  }
}
