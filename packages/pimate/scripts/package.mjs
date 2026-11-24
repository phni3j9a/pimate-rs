import { createHash } from 'node:crypto';
import { chmodSync, copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';

// Always stage from scratch: a missing target must never reuse an old binary.
const src = JSON.parse(readFileSync('package.json'));
const out = 'dist/package';
rmSync(out, { recursive: true, force: true });
mkdirSync(`${out}/dist`, { recursive: true });
copyFileSync('dist/extension.js', `${out}/dist/extension.js`);
copyFileSync('scripts/cli.js', `${out}/dist/cli.js`);
chmodSync(`${out}/dist/cli.js`, 0o755);
copyFileSync('LICENSE', `${out}/LICENSE`);
copyFileSync('../../README.md', `${out}/README.md`);
const sums = {};
for (const target of ['linux-x64', 'linux-arm64', 'darwin-x64', 'darwin-arm64']) {
  const dir = `bin/${target}`, path = `${dir}/pimate`;
  mkdirSync(`${out}/${dir}`, { recursive: true });
  copyFileSync(path, `${out}/${path}`);
  chmodSync(`${out}/${path}`, 0o755);
  sums[path] = createHash('sha256').update(readFileSync(`${out}/${path}`)).digest('hex');
}
writeFileSync(`${out}/bin/SHA256SUMS.json`, JSON.stringify(sums, null, 2) + '\n');
const { name, version, type, license, pi, dependencies, peerDependencies } = src;
writeFileSync(`${out}/package.json`, JSON.stringify({ name, version, type, license, pi, dependencies, peerDependencies,
  bin: { pimate: './dist/cli.js' }, keywords: ['pi-package'], engines: { node: '>=22.18.0' },
  publishConfig: { access: 'public' }, repository: 'github:phni3j9a/pimate-rs',
}, null, 2) + '\n');
