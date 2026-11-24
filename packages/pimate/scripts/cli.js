#!/usr/bin/env node
import { realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const target = `${process.platform}-${process.arch}`;
if (!['linux-x64', 'linux-arm64', 'darwin-x64', 'darwin-arm64'].includes(target)) {
  console.error(`PiMate Host does not support ${target}; use Linux or macOS on x64 or arm64.`);
  process.exit(1);
}
const host = realpathSync(fileURLToPath(new URL(`../bin/${target}/pimate`, import.meta.url)));
process.execve(host, [host, ...process.argv.slice(2)], process.env);
