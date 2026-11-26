// デスクトップ モックの共通部品（サイドバー・ヘッダー・入力欄）。?theme=dark でダーク。
if (new URLSearchParams(location.search).get('theme') === 'dark') document.documentElement.dataset.theme = 'dark';
const P = {
	search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
	plus: '<path d="M12 5v14M5 12h14"/>',
	gear: '<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9 7 7M17 17l2.1 2.1M4.9 19.1 7 17M17 7l2.1-2.1"/>',
	down: '<path d="m6 9 6 6 6-6"/>', right: '<path d="m9 6 6 6-6 6"/>', left: '<path d="m15 6-6 6 6 6"/>',
	more: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
	diff: '<rect x="4" y="3" width="16" height="18" rx="2"/><path d="M9 9h6M12 6v6M9 16h6"/>',
	agents: '<rect x="5" y="8" width="14" height="11" rx="3"/><path d="M12 4v4M9.5 13v1.5M14.5 13v1.5"/>',
	github: '<circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5v7M18 15.5V10a4 4 0 0 0-4-4h-3"/>',
	stop: '<circle cx="12" cy="12" r="9"/><rect x="9" y="9" width="6" height="6" rx="1" fill="currentColor"/>',
	up: '<path d="M12 19V5M6 11l6-6 6 6"/>',
	folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
	copy: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 0 1 2-2h9"/>',
	check: '<path d="m5 12 5 5 9-10"/>', x: '<path d="M6 6l12 12M18 6 6 18"/>',
	ext: '<path d="M14 4h6v6M20 4l-9 9M18 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4"/>',
	laptop: '<rect x="5" y="5" width="14" height="10" rx="1.5"/><path d="M3 19h18"/>',
	server: '<rect x="4" y="4" width="16" height="7" rx="1.5"/><rect x="4" y="13" width="16" height="7" rx="1.5"/><path d="M8 7.5h.01M8 16.5h.01"/>',
	side: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/>',
	panel: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/>',
	bell: '<path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 21h4"/>',
	pen: '<path d="M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4"/>',
	star: '<path d="m12 4 2.5 5.2 5.7.8-4.1 4 1 5.6-5.1-2.7-5.1 2.7 1-5.6-4.1-4 5.7-.8z"/>',
	bulb: '<path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.300 1 2.100h5c0-.8.4-1.600 1-2.100A6 6 0 0 0 12 3z"/>',
};
const I = (n, s = 16) => `<svg class="i" viewBox="0 0 24 24" style="width:${s}px;height:${s}px">${P[n]}</svg>`;
const W = (s) => document.write(s);

const titlebar = (t = 'PiMate') => W(`<div class="os">${t}</div>`);

// 一覧の単位は実行中の Pi Instance。要対応を最上段、その下にマシンごと。
const PIS = {
	login: ['wait', 'ログイン画面の改修', '<span class="waitc">質問: メールリンクの実装方式は？</span>', '今'],
	deps: ['fail', '依存パッケージの更新', '<span class="danger">失敗: npm ci が終了コード 1</span>', '3時間'],
	shout: ['spin', '--shout フラグを追加', '<span class="mono">~/code/demo</span>', '作業中'],
	auth: ['dot', '認証まわりのレビュー', '<span class="mono">~/work/api</span> · サブエージェント 2 実行中', '5分'],
	api: ['dot', 'API クライアントのリファクタ', '<span class="mono">~/work/api</span>', '2分'],
	readme: ['dot', 'README の整理', '<span class="mono">~/code/website</span> · スマホで起動', '1時間'],
	poc: ['dot', 'PiMate PoC', '<span class="mono">~/projects/PiMate</span>', '昨日'],
};
const pi = (id, o) => {
	const [d, name, sub, t] = PIS[id];
	const cls = ['pi', o.sel === id && 'sel', o.hover === id && 'hover', o.focus === id && 'focus'].filter(Boolean).join(' ');
	return `<div class="${cls}"><span class="${d === 'spin' ? 'spin' : 'dot ' + d}"></span><div class="col grow"><b class="trunc">${name}</b><span class="cap trunc">${sub}</span></div><span class="t">${t}</span></div>`;
};
const sidebar = (o = {}) => W(`<nav class="side"${o.style ? ` style="${o.style}"` : ''}>
	<div class="search${o.searchFocus ? ' focus' : ''}">${I('search')}<span class="grow">Pi・フォルダを検索</span><kbd>Ctrl K</kbd></div>
	<div class="grp">${I('bell', 13)}<span class="waitc">要対応</span></div>
	${pi('login', o)}${pi('deps', o)}
	<div class="grp">${I('laptop', 14)}MacBook<span class="state">接続中</span></div>
	${pi('shout', o)}${pi('auth', o)}${pi('api', o)}${pi('readme', o)}
	<div class="grp">${I('server', 14)}server-Macmini<span class="state">接続中</span></div>
	${pi('poc', o)}
	<div class="foot"><span class="tbtn plain${o.settings ? ' ink' : ''}">${I('gear')}設定</span><span class="btn">${I('plus')}新しい Pi</span></div>
</nav>`);

const head = (o = {}) => W(`<header class="head">
	${o.narrow ? `<span class="ib">${I('side')}</span>` : ''}
	<div class="col grow"><h1 class="trunc">${o.title ?? '--shout フラグを追加'}</h1><span class="meta trunc"><span class="mono" style="font-size:11px">${o.path ?? '~/code/demo'}</span> · ${o.machine ?? 'MacBook'}</span></div>
	<span class="status">${o.status ?? '<span class="spin"></span>作業中 · 1:04'}</span>
	${o.narrow ? '' : `<span class="ib${o.on === 'diff' ? ' on' : ''}">${I('diff')}</span><span class="ib${o.on === 'agents' ? ' on' : ''}">${I('agents')}</span><span class="ib${o.on === 'github' ? ' on' : ''}">${I('github')}</span>`}
	<span class="ib${o.on === 'more' ? ' on' : ''}">${I('more')}</span>
</header>`);

const composer = (o = {}) => W(`<div class="composer${o.focus ? ' focus' : ''}">
	<div class="text">${o.text ?? '<span class="ph">メッセージを入力　/ でコマンド　@ でファイル</span>'}</div>
	<div class="bar">
		<span class="ib">${I('plus')}</span>
		${o.running ? `<span class="chip${o.on === 'timing' ? ' on' : ''}">割り込む ${I('down', 12)}</span>` : ''}
		<span class="chip${o.on === 'model' ? ' on' : ''}">Opus 5.5 · high ${I('down', 12)}</span>
		<span class="grow"></span>
		<span class="chip${o.on === 'usage' ? ' on' : ''}" style="gap:6px"><span class="ring" style="--v:38%"></span><span class="ring" style="--v:62%"></span><span class="ring" style="--v:21%"></span></span>
		${o.running ? `<span class="ib" style="color:var(--text)">${I('stop', 22)}</span>` : ''}
		<span class="send${o.off ? ' off' : ''}">${I('up')}</span>
	</div>
</div>
${o.narrow ? '<div style="height:10px"></div>' : `<div class="hint"><span>Enter で送信</span><span>Shift+Enter で改行</span>${o.running ? '<span>Esc で停止</span>' : ''}<span style="margin-left:auto">Ctrl+K コマンド</span></div>`}`);

const notes = (title, items) => W(`<aside class="notes"><h3>${title}</h3>${items.map((s) => `<p>${s}</p>`).join('')}</aside>`);

const TOOLS = (hover) => `<div class="tool"><span class="dot done"></span><b>read</b> src/greet.ts <span class="r">42 行 ${I('right', 12)}</span></div>
	<div class="tool${hover ? ' hover' : ''}"><span class="dot done"></span><b>edit</b> src/greet.ts <span class="r">${hover ? `<span class="ink" style="font-family:inherit">差分を開く</span>${I('copy', 13)}` : ''}<span class="add">+6</span><span class="del">−2</span> ${I('right', 12)}</span></div>
	<div class="tool"><span class="dot done"></span><b>write</b> src/greet.test.ts <span class="r"><span class="add">+18</span> ${I('right', 12)}</span></div>
	<div class="tool"><span class="spin"></span><b>bash</b> npm test <span class="r">12秒 ${I('down', 12)}</span></div>
	<div class="out">&gt; vitest run
 ✓ src/greet.test.ts (3 tests)
 ⠹ src/cli.test.ts</div>`;
const transcript = (o = {}) => W(`<div class="scroll"><div class="column">
	<div class="me">プロジェクトの構成を説明して</div>
	<div class="sum">18秒 作業 · ツール 4 回 ${I('down', 12)}</div>
	<div>CLI のエントリは <code>src/cli.ts</code>、挨拶の生成は <code>src/greet.ts</code> にまとまっています。</div>
	<div class="meta mono" style="font-size:11px">claude-opus-5-5 · 6.2k in · 210 out · $0.04</div>
	<div class="me"><span class="ink mono">@src/greet.ts</span> に --shout フラグを追加して。テストも書いて</div>
	<div class="sum">${I('bulb', 13)} 4秒 考えた</div>
	${TOOLS(o.hover)}
</div></div>
${o.noQueue ? '' : `<div class="queue">${I('up', 13)}<span>完了後に送信</span><span class="trunc grow" style="color:var(--text)">README の使い方にも --shout の例を足して</span>${I('pen', 13)}${I('x', 13)}</div>`}`);
const chat = (o = {}) => { head(o); transcript(o); composer({ running: true, ...o }); };
