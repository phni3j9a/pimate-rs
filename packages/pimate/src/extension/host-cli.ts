import { execFile } from "node:child_process";
import { pipeline } from "node:stream/promises";
import { promisify } from "node:util";
import { cliPath } from "../package.ts";

const exec = promisify(execFile);
async function run<T>(args: string[], input?: string): Promise<T> {
	if (!process.env.PIMATE_HOST_BINARY && !(["linux", "darwin"].includes(process.platform) && ["x64", "arm64"].includes(process.arch))) {
		throw new Error(`PiMate Host does not support ${process.platform}-${process.arch}; use Linux or macOS on x64 or arm64.`);
	}
	try {
		const result = exec(cliPath, args, { timeout: 30_000 });
		const [{ stdout }] = await Promise.all([
			result,
			input === undefined ? undefined : pipeline([input], result.child.stdin!),
		]);
		return JSON.parse(stdout);
	} catch {
		// execFile errors can contain private arguments and pairing links.
		throw new Error(`PiMate Host command failed: ${args[0]}`);
	}
}
export const cli = <T>(...args: string[]) => run<T>(args);
export const pairPending = (code: string) => run<{ pending: boolean }>(["pair-pending"], code);
export type Device = { deviceId: string; name: string; lastSeenAt: number };
export const devices = () => cli<Device[]>("devices");
export const setupInfo = () => cli<{ host: { hostId: string }; configDir: string; addresses: string[] }>("setup-info");
export const service = (action: string) => cli("service", action);
export const serviceStatus = () => cli<{ installed: boolean; active: boolean; linger: string }>("service", "status");
export const commandStatus = () =>
	cli<{ path: string; dir: string; linked: boolean; conflict: boolean; onPath: boolean }>("service", "command-status");
export const hostHealth = () =>
	cli<{
		hostId: string;
		version: string;
		protocol: number;
		rcProtocol: number;
		addresses?: { address: string; port: number }[];
		incompatible?: unknown;
	} | null>("service", "health");
