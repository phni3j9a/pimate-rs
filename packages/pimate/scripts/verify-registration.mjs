import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const address = `127.0.0.1:${process.argv[2]}`;
const res = await fetch(`http://${address}/pair`, {
  method: 'POST', headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ code: readFileSync(0, 'utf8'), deviceName: 'smoke' }),
});
assert.equal(res.status, 200);
const { token } = await res.json();
const ws = new WebSocket(`ws://${address}/ws`);
ws.onopen = () => ws.send(JSON.stringify({ type: 'auth', protocol: 1, token, app: { platform: 'test', version: '1' } }));
let found = false;
ws.onmessage = ({ data }) => {
  const msg = JSON.parse(data);
  if (msg.type === 'instances' && msg.instances.some(i => i.instanceId === 'pack-smoke')) {
    found = true;
    ws.close();
  }
};
ws.onclose = () => assert.ok(found, 'Host did not report installed Pi');
