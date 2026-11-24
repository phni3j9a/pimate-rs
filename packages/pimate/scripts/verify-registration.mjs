import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

export async function verifyRegistration(address, code, timeout = 30_000) {
  const res = await fetch(`http://${address}/pair`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, signal: AbortSignal.timeout(timeout),
    body: JSON.stringify({ code, deviceName: 'smoke' }),
  });
  assert.equal(res.status, 200);
  const { token } = await res.json();
  const ws = new WebSocket(`ws://${address}/ws`);
  let timer;
  try {
    await new Promise((resolve, reject) => {
      timer = setTimeout(() => reject(Error('Host registration timed out')), timeout);
      ws.onerror = () => reject(Error('WebSocket connection failed'));
      ws.onclose = () => reject(Error('Host closed before reporting installed Pi'));
      ws.onopen = () => ws.send(JSON.stringify({ type: 'auth', protocol: 1, token, app: { platform: 'test', version: '1' } }));
      ws.onmessage = ({ data }) => {
        try {
          const msg = JSON.parse(data);
          if (msg.type === 'instances' && msg.instances.some(i => i.instanceId === 'pack-smoke')) resolve();
        } catch (e) { reject(e); }
      };
    });
  } finally {
    clearTimeout(timer);
    ws.close();
  }
}
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  await verifyRegistration(`127.0.0.1:${process.argv[2]}`, readFileSync(0, 'utf8'));
}
