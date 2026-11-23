// Optional `android-demo.py` fixture. Pairing, authentication, subscriptions and chat stay on
// its private Rust Host. Management/OAuth/GitHub are simulated; no subprocess, credentials or gh.
const http = require("node:http");
const WS = require("ws");
const upstream = Number(process.argv[2]);
if (
	!Number.isInteger(upstream) ||
	upstream <= 0 ||
	upstream > 65535 ||
	upstream === 47800
)
	throw Error("Expected the disposable Host port");
const host = "127.0.0.1";
const server = http.createServer((req, res) => {
	if (req.url !== "/pair" || req.method !== "POST")
		return res.writeHead(404).end();
	const post = http.request(
		{
			hostname: host,
			port: upstream,
			path: "/pair",
			method: "POST",
			headers: req.headers,
		},
		(r) => {
			res.writeHead(r.statusCode, r.headers);
			r.pipe(res);
		},
	);
	post.on("error", () => res.destroy());
	req.pipe(post);
});
const issue = (kind) => ({
	kind,
	number: 20,
	title: kind === "issue" ? "Android の残りの画面" : "画面の実装",
	state: "OPEN",
	author: "demo",
	updatedAt: "2026-11-20",
	labels: [],
	url: "https://github.com/example/demo",
});
// Opt-in connection-loss test: a second auth_list must come from a newly acquired hold.
let dropLogin = process.argv[3] === "reconnect";
new WS.Server({ server, path: "/ws" }).on("connection", (phone) => {
	const pc = new WS(`ws://${host}:${upstream}/ws`);
	let authed = false,
		updated = false,
		stored = false;
	let flow = null,
		waiting = null;
	const send = (data) => {
		if (phone.readyState === WS.OPEN) phone.send(JSON.stringify(data));
	};
	pc.on("error", () => phone.terminate());
	phone.on("error", () => pc.terminate());
	pc.on("close", () => phone.close());
	phone.on("close", () => pc.close());
	pc.on("message", (frame) => {
		const m = JSON.parse(frame);
		if (m.type === "auth_ok") {
			authed = true;
			m.host.hostRestart = true;
		}
		if (process.argv[3] === "empty" && m.type === "instances") m.instances = [];
		send(m);
	});
	phone.on("message", (frame) => {
		let m;
		try {
			m = JSON.parse(frame);
		} catch {
			phone.close();
			return;
		}
		const forward = () => pc.send(frame.toString());
		if (!authed) {
			if (m?.type !== "auth") return phone.close();
			if (pc.readyState === WS.OPEN) forward();
			else pc.once("open", forward);
			return;
		}
		const result = (body) =>
			send({ type: `${m.type}_result`, requestId: m.requestId, ...body });
		switch (m.type) {
			case "folders":
				return result({
					home: "/home/demo",
					folders: [
						{ path: "/home/demo/code/demo", running: 1 },
						{ path: "/home/demo/work/api", running: 0 },
					],
				});
			case "dirs":
				return result({ dirs: ["src", "test"] });
			case "update_check":
				return result({
					current: updated ? "0.1.20" : "0.1.19",
					latest: "0.1.20",
					canUpdate: true,
				});
			case "update":
				updated = true;
				return result({
					ok: true,
					from: "0.1.19",
					to: "0.1.20",
					restarting: false,
				});
			case "host_restart":
				return result({ ok: true });
			case "github_status":
				return result({
					ok: true,
					status: { installed: true, loggedIn: true, login: "demo" },
				});
			case "github":
				return result({
					ok: true,
					data:
						m.query.kind === "list"
							? {
									kind: "list",
									repo: {
										name: "example/demo",
										url: "https://github.com/example/demo",
									},
									issues: [issue("issue")],
									prs: [issue("pr")],
								}
							: {
									kind: "detail",
									item: {
										...issue(m.query.kind),
										...(m.query.kind === "pr"
											? { checksTruncated: true, checks: [] }
											: {}),
										body: "画面の動作と表示を確認します。",
										bodyTruncated: false,
										createdAt: "2026-11-20",
										comments: [],
										commentsTruncated: false,
									},
								},
				});
			case "auth_session":
				return result({ ok: true, instanceId: "demo-login" });
			case "auth_session_end":
				waiting = null;
				flow = null;
				return;
			case "rc":
				if (m.instanceId === "demo-login") {
					const { method, id } = m.request;
					const reply = (id, body) =>
						send({
							type: "rc_result",
							instanceId: "demo-login",
							response: { type: "response", ok: true, id, result: body },
						});
					if (method === "hello")
						return reply(id, {
							snapshot: {
								protocol: 1,
								instanceId: "demo-login",
								pid: 1,
								cwd: "/demo",
								origin: "mobile",
								abducoSession: null,
								startedAt: 1,
								session: null,
								status: "idle",
								providerAuth: true,
								model: null,
								thinking: null,
								pendingUI: [],
								seq: 1,
							},
						});
					if (method === "auth_list") {
						const provider =
							process.argv[3] === "reconnect" && !dropLogin
								? "anthropic"
								: "openai";
						if (dropLogin) {
							dropLogin = false;
							setTimeout(() => phone.close(), 1200);
						}
						return reply(id, {
							providers: [
								{
									id: provider,
									methods: ["api_key"],
									stored: stored ? "api_key" : null,
									configured: stored,
								},
							],
							...(flow ? { flow } : {}),
						});
					}
					if (method === "auth_wait") {
						if (m.request.params.afterRevision === flow?.revision) waiting = id;
						else reply(id, { flow });
						return;
					}
					if (method === "auth_begin")
						flow = {
							id: "demo",
							provider: "openai",
							method: "api_key",
							revision: 1,
							expiresAt: Date.now() + 300000,
							status: "input",
							prompt: { id: "key", kind: "api_key" },
						};
					else if (method === "auth_respond" || method === "auth_cancel") {
						stored = method === "auth_respond";
						flow = {
							...flow,
							status: stored ? "succeeded" : "cancelled",
							revision: 2,
						};
						delete flow.prompt;
					} else if (method === "auth_logout") {
						stored = false;
						flow = null;
						return reply(id, { refreshRequired: true });
					} else return phone.close();
					reply(id, { flow });
					if (waiting) {
						reply(waiting, { flow });
						waiting = null;
					}
					return;
				}
		}
		forward();
	});
});
server.listen(0, host, () => console.log(server.address().port));
