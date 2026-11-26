#!/usr/bin/env node
// html/*.html を PNG（ライト・ダーク）に書き出し、index.html を作り直す。
// 使い方: node docs/design/desktop/export.mjs [出力先]   （既定は png/。Git には入れない）
import { execFileSync } from 'node:child_process';
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const out = resolve(process.argv[2] ?? join(root, 'png'));
const chrome = process.env.CHROME ?? ['google-chrome-stable', 'google-chrome', 'chromium', 'chromium-browser',
	'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'].find((c) => {
	try { execFileSync(c, ['--version'], { stdio: 'ignore' }); return true; } catch { return false; }
});
if (!chrome) {
	console.error('Chrome / Chromium が見つかりません。CHROME=<実行ファイル> を指定してください。');
	process.exit(1);
}
const profile = join(tmpdir(), `pimate-desktop-export-${process.pid}`);
mkdirSync(out, { recursive: true });
const files = readdirSync(join(root, 'html')).filter((f) => f.endsWith('.html')).sort();
const cards = [];
for (const f of files) {
	const name = f.slice(0, -5);
	const title = /<title>(.*?)<\/title>/.exec(readFileSync(join(root, 'html', f), 'utf8'))?.[1] ?? name;
	for (const theme of ['light', 'dark']) {
		execFileSync(chrome, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run',
			`--user-data-dir=${profile}`, '--virtual-time-budget=3000', '--window-size=1320,1050', '--force-device-scale-factor=1.5',
			`--screenshot=${join(out, `${name}${theme === 'dark' ? '-dark' : ''}.png`)}`,
			`${pathToFileURL(join(root, 'html', f)).href}?theme=${theme}`], { stdio: 'ignore' });
	}
	cards.push(`<li><a href="html/${f}">${title}</a>　<a href="html/${f}?theme=dark">ダーク</a>　<code>${name}</code></li>`);
	console.log(name, title);
}
rmSync(profile, { recursive: true, force: true });
writeFileSync(join(root, 'index.html'), `<!DOCTYPE html>
<html lang="ja"><head><meta charset="utf-8"><title>PiMate デスクトップ 画面モック</title>
<!-- export.mjs が生成するファイル。直接編集しない -->
<style>body{font:14px/1.9 system-ui,sans-serif;max-width:760px;margin:40px auto;padding:0 20px;color:#1F2326}a{color:#155A7A}code{color:#5E686D}</style></head>
<body><h1 style="font-size:22px">PiMate デスクトップ 画面モック</h1>
<p>使い方は <a href="README.md">README.md</a>、現状の監査は <a href="AUDIT.md">AUDIT.md</a>、実装の段階は <a href="PLAN.md">PLAN.md</a>。</p>
<ul>
${cards.join('\n')}
</ul></body></html>
`);
console.log('index.html');
