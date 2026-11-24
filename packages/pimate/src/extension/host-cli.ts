import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { cliPath } from "../package.ts";

const exec = promisify(execFile);
export async function cli<T>(...args: string[]): Promise<T> {
	try {
		const { stdout } = await exec(cliPath, args, { timeout: 30_000 });
		return JSON.parse(stdout);
	} catch {
		// execFile errors can contain private arguments and pairing links.
		throw new Error(`PiMate Host command failed: ${args[0]}`);
	}
}
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
