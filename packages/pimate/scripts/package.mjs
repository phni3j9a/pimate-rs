import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
import { verifyBinary } from './binary-record.mjs';
import { chmodSync, cpSync, copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';

// Always stage from scratch: a missing target must never reuse an old binary.
const src = JSON.parse(readFileSync('package.json'));
const out = 'dist/package';
rmSync(out, { recursive: true, force: true });
mkdirSync(`${out}/dist`, { recursive: true });
copyFileSync('dist/extension.js', `${out}/dist/extension.js`);
copyFileSync('scripts/cli.js', `${out}/dist/cli.js`);
chmodSync(`${out}/dist/cli.js`, 0o755);
cpSync('grok-bot', `${out}/grok-bot`, { recursive: true });
copyFileSync('LICENSE', `${out}/LICENSE`);
copyFileSync('README.md', `${out}/README.md`);
const hostManifest = '../../crates/pimate-host/Cargo.toml';
const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--locked', '--format-version', '1']));
assert.equal(metadata.packages.find(p => p.name === 'pimate-host').version, src.version, 'Host/npm version mismatch');
mkdirSync(`${out}/notices`);
for (const file of ['T3-Tots.txt', 'musl.txt']) copyFileSync(`notices/${file}`, `${out}/notices/${file}`);
execFileSync('cargo-about', ['generate', '--locked', '--fail', '--manifest-path', hostManifest,
  '-c', 'notices/about.toml', 'notices/licenses.hbs', '-o', `${out}/notices/crates.txt`], { stdio: 'inherit' });
const sysroot = execFileSync('rustc', ['--print', 'sysroot'], { encoding: 'utf8' }).trim();
copyFileSync(`${sysroot}/share/doc/rust/COPYRIGHT-library.html`, `${out}/notices/rust-library.html`);
const sums = {};
for (const target of ['linux-x64', 'linux-arm64', 'darwin-x64', 'darwin-arm64']) {
  const dir = `bin/${target}`, path = `${dir}/pimate`;
  mkdirSync(`${out}/${dir}`, { recursive: true });
  copyFileSync(path, `${out}/${path}`);
  chmodSync(`${out}/${path}`, 0o755);
  sums[path] = verifyBinary(`${out}/${path}`, `${dir}/build.json`, target, src.version);
}
writeFileSync(`${out}/bin/SHA256SUMS.json`, JSON.stringify(sums, null, 2) + '\n');
const { name, version, type, license, pi, dependencies, peerDependencies } = src;
writeFileSync(`${out}/package.json`, JSON.stringify({ name, version, type, license, pi, dependencies, peerDependencies,
  bin: { pimate: './dist/cli.js' }, keywords: ['pi-package'], engines: { node: '^22.19.0 || >=23.11.0' },
  publishConfig: { access: 'public' }, repository: 'github:phni3j9a/pimate-rs',
}, null, 2) + '\n');
