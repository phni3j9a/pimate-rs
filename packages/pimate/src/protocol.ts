export type * from "../../../bindings/ts/index.ts";
import type { PromptImage, SubagentState, SubagentRun, SubagentCounts } from "../../../bindings/ts/index.ts";
export const PROTOCOL_VERSION = 1;
export const HOST_PROTOCOL_VERSION = 1;
export const DEFAULT_HOST_PORT = 47800;
export function slashCommandName(text: string): string | null {
	const token = text.split(/\s/, 1)[0]!;
	return /^\/[\w:.-]+$/.test(token) ? token.slice(1) : null;
}
export class LineDecoder {
	private buf = "";
	private readonly onValue: (value: unknown) => void;
	private readonly onError: (line: string) => void;
	private readonly maxLine: number;

	constructor(
		onValue: (value: unknown) => void,
		onError: (line: string) => void = () => {},
		maxLine = 16 * 1024 * 1024,
	) {
		this.onValue = onValue;
		this.onError = onError;
		this.maxLine = maxLine;
	}

	push(chunk: string): void {
		this.buf += chunk;
		let i = this.buf.indexOf("\n");
		while (i >= 0) {
			const line = this.buf.slice(0, i);
			this.buf = this.buf.slice(i + 1);
			if (line.trim()) {
				let value: unknown;
				let parsed = false;
				try {
					value = JSON.parse(line);
					parsed = true;
				} catch {
					this.onError(line);
				}
				if (parsed) this.onValue(value);
			}
			i = this.buf.indexOf("\n");
		}
		if (this.buf.length > this.maxLine) {
			this.onError(this.buf.slice(0, 200));
			this.buf = "";
		}
	}
}

export function encodeLine(value: unknown): string {
	return `${JSON.stringify(value)}\n`;
}

export const AUTH_PROVIDER_IDS = ["openai", "openai-codex", "anthropic", "opencode", "opencode-go"] as const;
export const AUTH_MAX_WAIT_MS = 25000;
export const AUTH_FLOW_TTL_MS = 15 * 60 * 1000;
export const AUTH_MAX_INPUT_LENGTH = 8192;
export const MAX_PROMPT_IMAGES = 4;
// 600 KiB * 4/3 base64 + 32 Ki UTF-16 units * 6 JSON escaping + envelope < 1 MiB.
export const MAX_PROMPT_TOTAL_IMAGE_BYTES = 600 * 1024;
export const MAX_PROMPT_IMAGE_BYTES = 512 * 1024;
export const MAX_IMAGE_PROMPT_TEXT = 32 * 1024;

export function validPromptImages(value: unknown): value is PromptImage[] {
	let total = 0;
	return (
		Array.isArray(value) &&
		value.length <= MAX_PROMPT_IMAGES &&
		value.every((image) => {
			if (
				!image ||
				typeof image !== "object" ||
				image.type !== "image" ||
				!["image/jpeg", "image/png", "image/webp"].includes(image.mimeType) ||
				typeof image.data !== "string"
			)
				return false;
			const data = image.data;
			if (
				!data.length ||
				data.length > Math.ceil(MAX_PROMPT_IMAGE_BYTES / 3) * 4 ||
				data.length % 4 !== 0 ||
				!/^[A-Za-z0-9+/]*={0,2}$/.test(data)
			)
				return false;
			const bytes = (data.length / 4) * 3 - (data.endsWith("==") ? 2 : data.endsWith("=") ? 1 : 0);
			total += bytes;
			return bytes <= MAX_PROMPT_IMAGE_BYTES && total <= MAX_PROMPT_TOTAL_IMAGE_BYTES;
		})
	);
}

const ACTIVE: ReadonlySet<SubagentState> = new Set(["queued", "running", "waiting", "stopping"]);

export function isSubagentActive(state: SubagentState): boolean {
	return ACTIVE.has(state);
}

export function countSubagents(runs: readonly SubagentRun[]): SubagentCounts {
	const counts: SubagentCounts = { active: 0, waiting: 0, paused: 0, failed: 0 };
	for (const run of runs) {
		const children = run.children.length > 0 ? run.children : [{ state: run.state }];
		for (const child of children) {
			if (child.state === "waiting") counts.waiting++;
			if (isSubagentActive(child.state)) counts.active++;
			else if (child.state === "paused") counts.paused++;
			else if (child.state === "failed") counts.failed++;
		}
	}
	return counts;
}
