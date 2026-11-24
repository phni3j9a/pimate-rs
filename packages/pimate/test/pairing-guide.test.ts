import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { afterEach, expect, it, vi } from "vitest";
import { hintSetup, registerSetup } from "../src/extension/setup.ts";
import { hasTailscaleAddress, pairedDeviceCount, pairingNotice, SETUP_HINT } from "../src/extension/pairing-guide.ts";
import { hostHealth, cli, pairPending, setupInfo, devices } from "../src/extension/host-cli.ts";
vi.mock("../src/extension/host-cli.ts");
const dirs: string[] = [];
const temp = () => {
	const dir = mkdtempSync(join(tmpdir(), "m7-"));
	dirs.push(dir);
	return dir;
};
afterEach(() => {
	vi.useRealTimers();
	vi.resetAllMocks();
	for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});
it("names the app's scan button and explains a missing Tailscale address with the commands to recover", () => {
	expect(hasTailscaleAddress(["127.0.0.1", "100.101.102.103"])).toBe(true);
	expect(hasTailscaleAddress(["127.0.0.1", "fd7a:115c:a1e0::1"])).toBe(true);
	expect(hasTailscaleAddress(["127.0.0.1"])).toBe(false);
	const ok = pairingNotice(["127.0.0.1", "100.101.102.103", "fd7a:115c:a1e0::1"], 47800, "pimate://pair?x");
	expect(ok).toContain("Listening: 127.0.0.1:47800, 100.101.102.103:47800, [fd7a:115c:a1e0::1]:47800");
	expect(ok).toContain('tap "QRコードを読み取る"');
	expect(ok).toContain("pimate://pair?x");
	expect(ok).not.toContain("WARNING");
	const loopback = pairingNotice(["127.0.0.1"], 47800, "pimate://pair?x");
	expect(loopback).toContain("WARNING: No Tailscale address");
	expect(loopback).toContain("same account as the phone");
	expect(loopback).toContain("/pimate restart and /pimate pair");
});

it("counts paired devices without creating the config directory", () => {
	const dir = join(temp(), "pimate");
	expect(pairedDeviceCount(dir)).toBe(0);
	expect(existsSync(dir)).toBe(false);
});

it("hints /pimate setup once per process, only in the PC TUI of an unpaired PC without a Host", async () => {
	const notify = vi.fn();
	const ctx = (mode: string) => ({ mode, ui: { notify } }) as unknown as ExtensionContext;
	const unpaired = { configDir: () => join(temp(), "none"), health: vi.fn(async () => null) };

	for (const mode of ["rpc", "json", "text"])
		await hintSetup(ctx(mode), { setupHinted: false, abducoSession: null }, unpaired);
	await hintSetup(ctx("tui"), { setupHinted: false, abducoSession: "pimate-1" }, unpaired);
	expect(unpaired.health).not.toHaveBeenCalled();

	const running = { ...unpaired, health: vi.fn(async () => ({ ok: true }) as never) };
	await hintSetup(ctx("tui"), { setupHinted: false, abducoSession: null }, running);
	expect(notify).not.toHaveBeenCalled();

	const dir = temp();
	writeFileSync(join(dir, "devices.json"), "[{}]");
	const paired = { configDir: () => dir, health: vi.fn(async () => null) };
	await hintSetup(ctx("tui"), { setupHinted: false, abducoSession: null }, paired);
	expect(paired.health).not.toHaveBeenCalled();

	const state = { setupHinted: false, abducoSession: null };
	await hintSetup(ctx("tui"), state, unpaired);
	await hintSetup(ctx("tui"), state, unpaired);
	expect(notify).toHaveBeenCalledTimes(1);
	expect(notify).toHaveBeenCalledWith(SETUP_HINT, "info");
});

it("the QR screen notices the phone's pairing, then shows the next step instead of the code", async () => {
	vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
	let paired = false;
	vi.mocked(setupInfo).mockResolvedValue({ host: { hostId: "id" }, configDir: "unused", addresses: [] });
	vi.mocked(devices).mockImplementation(async () =>
		paired ? [{ deviceId: "d", name: "Pixel 8", lastSeenAt: 0 }] : [],
	);
	vi.mocked(pairPending).mockImplementation(async (code) => {
		expect(code).toBe("private");
		return { pending: !paired };
	});
	vi.mocked(cli).mockImplementation(async (...args) => {
		expect(args).toEqual(["pair", "--json", "--address", "100.101.102.103:47801"]);
		return { code: "private", link: "pimate://pair?code=private", expiresAt: Date.now() + 60000 };
	});
	vi.mocked(hostHealth).mockResolvedValue({
		ok: true,
		protocol: 1,
		hostId: "id",
		addresses: [
			{ address: "127.0.0.1", port: 47801 },
			{ address: "100.101.102.103", port: 47801 },
		],
	} as never);
	const registerCommand = vi.fn();
	registerSetup({ registerCommand } as unknown as ExtensionAPI);
	const handler = registerCommand.mock.calls[0][1].handler;
	type Screen = { render(width: number): string[]; handleInput(data: string): void; dispose(): void };
	let screen: Screen | undefined;
	const requestRender = vi.fn();
	let closed = false;
	const custom = vi.fn(
		(factory: (tui: unknown, theme: unknown, kb: unknown, done: () => void) => Screen) =>
			new Promise<void>((resolve) => {
				screen = factory({ requestRender }, {}, {}, () => {
					closed = true;
					resolve();
				});
			}),
	);
	const running = handler("pair", { mode: "tui", ui: { custom, notify: vi.fn() } });
	await vi.waitFor(() => expect(screen).toBeDefined());
	const waiting = screen!.render(200).join("\n");
	expect(waiting).toContain('tap "QRコードを読み取る"');
	expect(waiting).toContain("Waiting for your phone");
	expect(waiting).toContain("pimate://pair?code=private");

	await vi.advanceTimersByTimeAsync(3000);
	expect(requestRender).not.toHaveBeenCalled();
	paired = true;
	await vi.advanceTimersByTimeAsync(1000);
	expect(requestRender).toHaveBeenCalledTimes(1);
	const result = screen!.render(200).join("\n");
	expect(result).toContain('Paired with "Pixel 8".');
	expect(result).not.toContain("pimate://pair");
	await vi.advanceTimersByTimeAsync(5000);
	expect(requestRender).toHaveBeenCalledTimes(1);
	screen!.handleInput("\r");
	await running;
	expect(closed).toBe(true);
});
