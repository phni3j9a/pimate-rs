import { mkdtempSync, rmSync, writeFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it, vi } from "vitest";

it("sends pairing secrets only on stdin and suppresses child output on errors", async () => {
	const dir = mkdtempSync(join(tmpdir(), "pair-pipe-"));
	const exe = join(dir, "pimate");
	const args = join(dir, "argv");
	const code = "private-lookup-code";
	const stdout = vi.spyOn(process.stdout, "write");
	const stderr = vi.spyOn(process.stderr, "write");
	try {
		writeFileSync(
			exe,
			`#!/bin/sh\nprintf '%s\\n' "$@" > "${args}"\nv=$(cat)\nif [ -e "${dir}/fail" ]; then printf '%s' "$v" >&2; exit 1; fi\n[ "$v" = '${code}' ] && echo '{"pending":true}'\n`,
			{ mode: 0o700 },
		);
		vi.stubEnv("PIMATE_HOST_BINARY", exe);
		const { pairPending } = await import("../src/extension/host-cli.ts");
		expect(await pairPending(code)).toEqual({ pending: true });
		expect(readFileSync(args, "utf8")).toBe("pair-pending\n");
		writeFileSync(join(dir, "fail"), "");
		await expect(pairPending(code)).rejects.toThrow(/^PiMate Host command failed: pair-pending$/);
		expect([...stdout.mock.calls, ...stderr.mock.calls].flat().map(String).join("")).not.toContain(code);
	} finally {
		stdout.mockRestore();
		stderr.mockRestore();
		vi.unstubAllEnvs();
		rmSync(dir, { recursive: true, force: true });
	}
});
