import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

export function binaryRecord(path, target, version) {
  const changes = execFileSync('git', ['status', '--porcelain', '--untracked-files=all', '--',
    ...['Cargo.toml', 'Cargo.lock', 'crates', 'vendor', '.cargo', 'rust-toolchain*'].map(path => `:(top)${path}`)], { encoding: 'utf8' });
  assert.equal(changes, '', `Commit or revert Host build inputs before packaging:\n${changes}`);
  return { target, version, tree: execFileSync('git', ['rev-parse', 'HEAD^{tree}'], { encoding: 'utf8' }).trim(),
    sha256: createHash('sha256').update(readFileSync(path)).digest('hex') };
}

export function verifyBinary(path, recordPath, target, version) {
  const expected = binaryRecord(path, target, version);
  assert.deepEqual(JSON.parse(readFileSync(recordPath)), expected, `Host artifact mismatch: ${target}`);
  return expected.sha256;
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [path, target] = process.argv.slice(2);
  const meta = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--format-version', '1', '--no-deps']));
  console.log(JSON.stringify(binaryRecord(path, target, meta.packages.find(p => p.name === 'pimate-host').version)));
}
