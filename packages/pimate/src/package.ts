import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// Both src/package.ts and the bundled dist/extension.js are one level below the manifest.
export const PACKAGE_VERSION: string = JSON.parse(
	readFileSync(new URL("../package.json", import.meta.url), "utf8"),
).version;
export const cliPath =
	process.env.PIMATE_HOST_BINARY ||
	fileURLToPath(new URL(`../bin/${process.platform}-${process.arch}/pimate`, import.meta.url));
