import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { binaryRecord, verifyBinary } from './binary-record.mjs';
import { verifyRegistration } from './verify-registration.mjs';

test('pack rejects stale or mixed artifacts, even for the same version', t => {
  const dir = mkdtempSync(join(tmpdir(), 'host-record-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const host = join(dir, 'pimate'), json = join(dir, 'build.json');
  writeFileSync(host, 'test');
  const record = binaryRecord(host, 'linux-arm64', '1.0.0');
  const verify = () => verifyBinary(host, json, 'linux-arm64', '1.0.0');
  writeFileSync(json, JSON.stringify(record));
  assert.equal(verify(), record.sha256);
  for (const key of Object.keys(record)) {
    writeFileSync(json, JSON.stringify({ ...record, [key]: 'stale' }));
    assert.throws(verify, /Host artifact mismatch/);
  }
});

for (const scenario of ['upgrade failure', 'silent Host']) {
  test(`registration rejects ${scenario}`, async t => {
    const server = createServer((req, res) => res.end('{"token":"test"}'));
    server.on('upgrade', (_, socket) => {
      if (scenario === 'upgrade failure') socket.end('HTTP/1.1 404 Not Found\r\n\r\n');
      else t.after(() => socket.destroy());
    });
    server.listen(0, '127.0.0.1');
    await once(server, 'listening');
    t.after(() => { server.close(); server.closeAllConnections(); });
    await assert.rejects(verifyRegistration(`127.0.0.1:${server.address().port}`, 'test', 1000),
      scenario === 'upgrade failure' ? /WebSocket connection failed/ : /registration timed out/);
  });
}
