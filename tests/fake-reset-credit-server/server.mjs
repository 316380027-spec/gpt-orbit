#!/usr/bin/env node
import { createServer } from 'node:http';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const scenario = process.env.GPT_ORBIT_RESET_SCENARIO ?? 'live';
const stateDir = process.env.GPT_ORBIT_RESET_STATE_DIR;
const supported = new Set([
  'live',
  'zero',
  'disconnect',
  'malformed',
  'unauthorized',
  'oversized',
  'redirect',
  'recovery',
  'service-sequence',
]);

if (!stateDir || !supported.has(scenario)) {
  process.stderr.write('invalid fake reset-credit configuration\n');
  process.exit(2);
}

mkdirSync(stateDir, { recursive: true });
let requestCount = 0;

const server = createServer((request, response) => {
  requestCount += 1;
  writeFileSync(join(stateDir, 'request-count'), String(requestCount));
  writeFileSync(
    join(stateDir, 'header-names.json'),
    JSON.stringify(Object.keys(request.headers).map((name) => name.toLowerCase()).sort()),
  );

  if (request.method !== 'GET' || request.url !== '/reset-credits') {
    sendJson(response, 404, { error: 'not-found' });
    return;
  }

  switch (scenario) {
    case 'live':
      sendJson(response, 200, { available_count: 3 });
      return;
    case 'zero':
      sendJson(response, 200, { available_count: 0 });
      return;
    case 'disconnect':
      request.socket.destroy();
      return;
    case 'malformed':
      response.writeHead(200, { 'Content-Type': 'application/json' });
      response.end('{invalid-json');
      return;
    case 'unauthorized':
      sendJson(response, 401, { error: 'unauthorized' });
      return;
    case 'oversized':
      sendJson(response, 200, { available_count: 3, padding: 'x'.repeat(65_536) });
      return;
    case 'redirect': {
      const { port } = server.address();
      response.writeHead(302, { Location: `http://127.0.0.1:${port}/redirect-target` });
      response.end();
      return;
    }
    case 'recovery':
      sendJson(response, 200, { available_count: 2 });
      return;
    case 'service-sequence':
      if (requestCount === 1) {
        sendJson(response, 200, { available_count: 3 });
        return;
      }
      if (requestCount === 2) {
        request.socket.destroy();
        return;
      }
      sendJson(response, 200, { available_count: 2 });
      return;
  }
});

server.on('error', () => {
  process.stderr.write('fake reset-credit server failed\n');
  process.exitCode = 1;
});

server.listen(0, '127.0.0.1', () => {
  const address = server.address();
  if (!address || typeof address === 'string' || address.address !== '127.0.0.1') {
    process.stderr.write('fake reset-credit server did not bind loopback\n');
    process.exit(1);
  }
  writeFileSync(join(stateDir, 'port'), String(address.port));
});

function sendJson(response, status, body) {
  response.writeHead(status, { 'Content-Type': 'application/json' });
  response.end(JSON.stringify(body));
}
