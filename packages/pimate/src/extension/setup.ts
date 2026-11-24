import type { ExtensionAPI, ExtensionCommandContext, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { Key, matchesKey, Text } from "@earendil-works/pi-tui";
import { DEFAULT_HOST_PORT, HOST_PROTOCOL_VERSION, PROTOCOL_VERSION } from "../protocol.ts";
import QRCode from "qrcode";
import { homedir } from "node:os";
import { join } from "node:path";
import {
	cli,
	commandStatus,
	devices as listDevices,
	hostHealth,
	service,
	serviceStatus,
	setupInfo,
} from "./host-cli.ts";

import {
	hasTailscaleAddress,
	NO_TAILSCALE,
	type PairingProgress,
	formatAddress,
	printable,
	pairedDeviceCount,
	pairingNotice,
	pairingProgress,
	pairingStatusLines,
	SETUP_HINT,
} from "./pairing-guide.ts";
import { cliPath, PACKAGE_VERSION } from "../package.ts";
import { getLocalUI } from "./ui.ts";

/** `pi install` keeps package bins off PATH, so `pimate attach` shown on the phone needs this link. */
async function offerCommand(ctx: ExtensionCommandContext): Promise<void> {
	if (process.platform === "win32") return;
	const ui = getLocalUI(ctx);
	const cmd = await commandStatus();
	if (cmd.conflict) {
		ui.notify(
			`${cmd.path} exists and is not PiMate's link; left unchanged. Run ${cliPath} directly instead.`,
			"warning",
		);
		return;
	}
	if (!cmd.linked) {
		if (
			!(await ui.confirm(
				"Add the pimate command?",
				`Link ${cmd.path} -> ${cliPath} so commands shown on the phone (pimate attach …) work in a terminal? Shell config files are not changed.`,
			))
		)
			return;
		await service("command-install");
		ui.notify(`Installed ${cmd.path}`, "info");
	}
	if (!cmd.onPath)
		ui.notify(
			`${cmd.dir} is not on PATH. Add this to your shell config, then open a new terminal:\nexport PATH="${cmd.dir}:$PATH"`,
			"warning",
		);
}

async function pair(ctx: ExtensionCommandContext): Promise<void> {
	const ui = getLocalUI(ctx);
	const health = await hostHealth();
	if (!health) throw new Error("Host is stopped. Run /pimate setup or /pimate start first.");
	const { host } = await setupInfo();
	if (health.hostId !== host.hostId) throw new Error("Another Host is listening; stop it first.");
	const addresses = health.addresses?.map((a) => a.address) ?? ["127.0.0.1"];
	const port = health.addresses?.[0]?.port ?? DEFAULT_HOST_PORT;
	const address = addresses.find((ip) => hasTailscaleAddress([ip])) ?? "127.0.0.1";
	const before = new Set((await listDevices()).map((d) => d.deviceId));
	const { code, link, expiresAt } = await cli<{ code: string; link: string; expiresAt: number }>(
		"pair",
		"--json",
		"--address",
		`${formatAddress(address)}:${port}`,
	);
	const qr = await QRCode.toString(link, { type: "utf8", errorCorrectionLevel: "L" });
	const notice = pairingNotice(addresses, port, link);
	await ui.custom<void>((tui, _theme, _kb, done) => {
		// Only the Rust Host owns credentials and writes pairing state.
		let progress: PairingProgress = { state: "waiting" };
		let checking = false;
		const check = async () => {
			if (checking) return;
			checking = true;
			try {
				progress = pairingProgress({
					codePending: (await cli<{ pending: boolean }>("pair-pending", code)).pending,
					before,
					devices: await listDevices(),
					expiresAt,
					now: Date.now(),
				});
			} catch {
				return;
			} finally {
				checking = false;
			}
			if (progress.state === "waiting") return;
			clearInterval(timer);
			tui.requestRender();
		};
		const timer = setInterval(check, 1000);
		timer.unref();
		const text = (lines: string[], width: number) => new Text(lines.join("\n"), 0, 0).render(width);
		return {
			render(width) {
				// Once used or expired, the code is worthless: show the outcome instead of the QR.
				if (progress.state !== "waiting") return text(pairingStatusLines(progress), width);
				const lines = qr.trimEnd().split("\n");
				return [
					...text([notice], width),
					...(width >= Math.max(...lines.map((line) => line.length))
						? lines
						: text(["Widen the terminal to show the QR, or use the link above."], width)),
					...text(pairingStatusLines(progress), width),
				];
			},
			handleInput(data) {
				if (!matchesKey(data, Key.enter) && !matchesKey(data, Key.escape)) return;
				clearInterval(timer);
				done();
			},
			invalidate() {},
			dispose() {
				clearInterval(timer);
			},
		};
	});
}
/**
 * After `pi install`, nothing tells a PC user about `/pimate setup`. A PC TUI Pi says it once per process while
 * no phone has ever paired and no Host runs; it never creates config files or starts anything.
 */
export function hintSetup(
	ctx: ExtensionContext,
	state: { setupHinted: boolean; abducoSession: string | null },
	deps = {
		configDir: () =>
			process.env.PIMATE_CONFIG_DIR || join(process.env.XDG_CONFIG_HOME || join(homedir(), ".config"), "pimate"),
		health: hostHealth,
	},
): Promise<void> {
	if (ctx.mode !== "tui" || state.setupHinted || state.abducoSession) return Promise.resolve();
	state.setupHinted = true;
	if (pairedDeviceCount(deps.configDir()) > 0) return Promise.resolve();
	return deps
		.health()
		.catch(() => null)
		.then((health) => {
			if (health) return;
			try {
				getLocalUI(ctx).notify(SETUP_HINT, "info");
			} catch {
				// The session was replaced meanwhile (/new, /resume): its context is gone.
			}
		});
}

export function registerSetup(pi: ExtensionAPI): void {
	pi.registerCommand("pimate", {
		description: "PiMate: setup, pair, devices, status, start, stop, restart, uninstall (PC only)",
		handler: async (args, ctx) => {
			if (ctx.mode !== "tui") {
				ctx.ui.notify("/pimate is available only in the PC TUI", "error");
				return;
			}
			const ui = getLocalUI(ctx);
			try {
				switch (args.trim() || "status") {
					case "setup": {
						const status = await serviceStatus();
						if (await hostHealth()) {
							await offerCommand(ctx);
							if (await ui.confirm("PiMate is already running", "Create another phone pairing?")) await pair(ctx);
							return;
						}
						const tailscale = hasTailscaleAddress((await setupInfo()).addresses);
						if (
							!(await ui.confirm(
								"Set up PiMate",
								`Start a Host listening on loopback and this PC's Tailscale addresses, then create a private phone pairing code? Paired devices can control Pi. No LAN/public listener is opened.${tailscale ? "" : `\n\n${NO_TAILSCALE}. You can continue now and pair later with /pimate restart and /pimate pair.`}`,
							))
						)
							return;
						if (status.installed) {
							await service("start");
						} else if (
							(await cli<boolean>("service", "has-systemd")) &&
							(await ui.confirm(
								"Register systemd user service?",
								`Create pimate-host.service and enable --now (starts at login; survives Pi exit)? Linger=${status.linger}. Linger is NOT changed. Choose No to start only for now, without installing a service.`,
							))
						) {
							await service("install");
						} else {
							ui.notify(
								"Starting for now, without service registration. Non-Linux service registration is unsupported; detached fallback may stop at logout.",
								"info",
							);
							await service("start");
						}
						await offerCommand(ctx);
						await pair(ctx);
						return;
					}
					case "pair":
						await pair(ctx);
						return;
					case "devices": {
						const devices = await listDevices();
						if (!devices.length) {
							ui.notify("No paired devices", "info");
							return;
						}
						const labels = devices.map(
							(d) =>
								`${printable(d.name)} — ${d.deviceId} (last seen: ${d.lastSeenAt ? new Date(d.lastSeenAt).toISOString() : "never"})`,
						);
						const chosen = await ui.select("Paired devices — select to revoke, Esc to close", labels);
						if (chosen && (await ui.confirm("Revoke device?", chosen))) {
							await cli("revoke", devices[labels.indexOf(chosen)].deviceId, "--json");
							ui.notify("Device revoked", "info");
						}
						return;
					}
					case "status": {
						const health = await hostHealth();
						const status = await serviceStatus();
						const cmd = await commandStatus();
						ui.notify(
							`PiMate Extension ${PACKAGE_VERSION} / RC protocol ${PROTOCOL_VERSION}\nHost: ${health ? `${health.version ?? "unknown"} / protocol ${health.protocol} / RC ${health.rcProtocol ?? "unknown"}` : "stopped"}\nService: installed=${status.installed}, active=${status.active}, linger=${status.linger}\nListen: ${health?.addresses?.map((a) => `${a.address.includes(":") ? `[${a.address}]` : a.address}:${a.port}`).join(", ") ?? "unknown"}\nSkipped incompatible RCs: ${JSON.stringify(health?.incompatible ?? {})}\nCommand: ${cmd.path} (${cmd.linked ? "linked" : cmd.conflict ? "other file, not PiMate's" : "not installed; run /pimate setup"}${cmd.onPath ? "" : ", directory not on PATH"})`,
							"info",
						);
						if (
							health &&
							(health.version !== PACKAGE_VERSION ||
								health.protocol !== HOST_PROTOCOL_VERSION ||
								health.rcProtocol !== PROTOCOL_VERSION)
						) {
							if (
								await ui.confirm(
									"Host / extension version mismatch",
									"Restart Host with the current package? Existing phone connections will reconnect.",
								)
							) {
								await service("stop");
								if (status.installed) await service("install");
								else await service("start");
							}
						}
						return;
					}
					case "start":
						if (
							!(await hostHealth()) &&
							!(await ui.confirm("Start PiMate Host?", "Listen on loopback + Tailscale and accept paired phones?"))
						)
							return;
						await service("start");
						ui.notify("Host started", "info");
						return;
					case "stop":
						await service("stop");
						ui.notify("Host stopped (any installed login service is kept)", "info");
						return;
					case "restart":
						if (await ui.confirm("Restart Host?", "Disconnect phones briefly and use this installed PiMate version?")) {
							const installed = (await serviceStatus()).installed;
							await service("stop");
							if (installed) await service("install");
							else await service("start");
							ui.notify("Host restarted", "info");
						}
						return;
					case "uninstall":
						if (
							await ui.confirm(
								"Remove PiMate Host service?",
								"Stop Host and remove its systemd user unit and PiMate's pimate command link. Paired device config and the Pi package are kept.",
							)
						) {
							await service("uninstall");
							await service("command-remove");
							ui.notify(
								`Host service and pimate command link removed. Run: pi remove npm:@phni3j9a/pimate\nThen optionally delete the PiMate config directory to forget all pairings. For local installs use pi remove <installed-directory>.`,
								"info",
							);
						}
						return;
					default:
						ui.notify("Usage: /pimate setup|pair|devices|status|start|stop|restart|uninstall", "info");
				}
			} catch (error) {
				ui.notify(String(error), "error");
			}
		},
	});
}
