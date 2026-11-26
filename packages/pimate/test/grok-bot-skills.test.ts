import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const skills = new URL("../grok-bot/skills/", import.meta.url);
const cli = readFileSync(new URL("../../../crates/pimate-host/src/main.rs", import.meta.url), "utf8");
const usage = cli.match(/const USAGE: &str = "Usage: pimate [^\n]+"/)?.[0] ?? "";
const commands = new Set(Array.from(usage.matchAll(/(?:Usage: pimate | \| )([a-z]+)/g), (match) => match[1]));
const webhookActions = new Set(usage.match(/webhook <([a-z|]+)>/)?.[1].split("|"));

describe("Grok Bot skills", () => {
	it("never requests a pending snapshot from the webhook card flow", () => {
		const cards = readFileSync(new URL("pimate-phone-answers/SKILL.md", skills), "utf8");
		expect(cards).not.toMatch(/\bpimate wait\b|--wait\b/);
	});
	it("has matching frontmatter and uses only real CLI subcommands", () => {
		const folders = readdirSync(skills, { withFileTypes: true }).filter((entry) => entry.isDirectory());
		expect(folders.map((entry) => entry.name).sort()).toEqual([
			"pimate-getting-started",
			"pimate-phone-answers",
			"pimate-rules",
			"pimate-send",
		]);
		expect(commands.size).toBeGreaterThan(0);
		expect(webhookActions.size).toBeGreaterThan(0);
		for (const folder of folders) {
			const text = readFileSync(join(fileURLToPath(skills), folder.name, "SKILL.md"), "utf8");
			for (const [snippet] of text.matchAll(/`[^`\n]*`/g)) {
				if (/\bpimate wait\b|--wait\b/.test(snippet)) expect(snippet).toContain("--redact");
			}
			const frontmatter = text.match(/^---\n([\s\S]*?)\n---\n/)?.[1] ?? "";
			expect(frontmatter.match(/^name:\s*(\S+)\s*$/m)?.[1]).toBe(folder.name);
			expect(frontmatter.match(/^description:\s*(\S.*)$/m)?.[1].trim()).toBeTruthy();
			// `/pimate setup` etc. are Pi's slash commands, not the CLI.
			for (const match of text.matchAll(/(?<!\/)\bpimate ([a-z][a-z-]*)\b/g)) expect(commands).toContain(match[1]);
			for (const match of text.matchAll(/\bpimate webhook ([a-z]+)/g)) expect(webhookActions).toContain(match[1]);
		}
	});
});
