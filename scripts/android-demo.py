#!/usr/bin/env python3
"""A disposable, loopback-only Rust Host plus in-memory NDJSON RCs.

Instances mirror the design mocks (docs/design): `demo` streams Markdown and takes prompts,
`main` stays at work with a running tool, `waiting` holds a question, `edge` is switched to
another session "on the PC" by SIGUSR1, `attach` waits on a PC-only screen (SIGUSR2 ends it),
`images` shows pictures, the rest fill the list.
"""

import asyncio
import base64
import itertools
import json
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import tempfile
import time
import zlib

ROOT = Path(__file__).resolve().parents[1]
BODY = (
    "## 開発用の会話\n\n| 項目 | 状態 |\n| --- | --- |\n| Android | 接続済み |\n\n"
    '```kotlin\nval greeting = "こんにちは"\nfun greet(name: String, shout: Boolean = false): String'
    ' = if (shout) "$greeting、$name！".uppercase() else "$greeting、$name"\n```\n\n'
    + "日本語の長い文章と折り返しを確認します。" * 6
    + "\n\n### 表\n\n"
    "| ファイル | 変更内容 |\n| --- | --- |\n"
    "| `src/components/Greeting.tsx` | 挨拶の文言を引数で受け取り、`shout` が真のときは大文字にして返すように変更しました |\n"
    "| `src/cli.ts` | `--shout` フラグを追加し、ヘルプにも説明を載せました |\n\n"
    "| 名前 | 種類 | 行数 | 追加 | 削除 | 備考 |\n| --- | --- | ---: | ---: | ---: | --- |\n"
    "| `greet.ts` | 実装 | 42 | 6 | 2 | 引数を追加 |\n"
    "| `greet.test.ts` | テスト | 18 | 18 | 0 | **最後のセル** |\n\n"
    "### リスト\n\n- `LoginForm.tsx` はパスワード入力のみ\n- 認証は `/api/session` に集約\n\n"
    "1. 依存を入れる\n2. テストを通す\n\n- [x] 実装\n- [ ] 使い方を書く\n\n"
    "> 引用: 変更は 3 ファイル程度です。\n\n"
    "[PiMate](https://github.com/phni3j9a/pimate-rs)"
)
PATCH = (
    "--- a/src/greet.ts\n+++ b/src/greet.ts\n@@ -1,4 +1,8 @@\n"
    "-export function greet(name: string) {\n-  return `Hello, ${name}`;\n"
    "+export function greet(name: string, shout = false) {\n+  const text = `Hello, ${name}`;\n"
    "+  return shout ? text.toUpperCase() : text;\n+}\n+\n+export const DEFAULT = 'world';\n"
)
NOW = int(time.time() * 1000)
MODEL = dict(provider="anthropic", id="claude-opus-5-5")
LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh"]
MODELS = [
    dict(provider=p, id=i, name=n, contextWindow=w, thinkingLevels=levels)
    for p, i, n, w, levels in [
        ("anthropic", "claude-opus-5-5", "Claude Opus 5.5", 200_000, LEVELS),
        ("anthropic", "claude-sonnet-5-5", "Claude Sonnet 5.5", 1_000_000, LEVELS),
        ("anthropic", "claude-haiku-5-5", "Claude Haiku 5.5", 200_000, ["off"]),
        ("openai", "gpt-5-codex", "GPT-5 Codex", 400_000, LEVELS[:5]),
        ("openai", "gpt-5", "GPT-5", 400_000, LEVELS[:5]),
        ("google", "gemini-3-pro", "Gemini 3 Pro", 1_000_000, ["off", "low", "high"]),
        ("google", "gemini-3-flash", "Gemini 3 Flash", 1_000_000, ["off"]),
        ("xai", "grok-5", "Grok 5", 256_000, ["off", "low", "high"]),
    ]
]
COMMANDS = [
    dict(name="review", description="変更をレビューする", source="extension"),
    dict(name="fix-tests", description="失敗したテストを直す", source="prompt"),
    dict(name="skill:release-notes", description="リリースノートを書く", source="skill"),
]
HOUR = 3_600_000


def png(width, height, color, stripe=(255, 255, 255)):
    """A small picture without extra dependencies: a flat color with a diagonal stripe."""
    rows = b"".join(
        b"\0"
        + b"".join(
            bytes(stripe if (x + y) % 40 < 6 else color) for x in range(width)
        )
        for y in range(height)
    )

    def chunk(kind, data):
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data))
        )

    data = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )
    return dict(mimeType="image/png", data=base64.b64encode(data).decode())


COLORS = [(76, 147, 188), (243, 140, 123), (245, 193, 94), (94, 104, 109), (27, 107, 145)]


def picture(index, thumbnail=False):
    w, h = (160, 100) if thumbnail else (800, 500)
    return png(w, h, COLORS[index % len(COLORS)])


def preview(index):
    return dict(type="image", **picture(index, True))


async def pi(
    run,
    iid,
    cwd,
    *,
    name=None,
    origin="pc",
    status="idle",
    age=0,
    seed=(),
    pending=(),
    runs=(),
    abduco=None,
    context=None,
    quotas=(),
    pc_prompt=None,
    other=(),
    hooks=None,
):
    """One fake Pi: `<iid>.sock` + `<iid>.json` in the Host's run dir.

    `other`: saved sessions as (name, seed) for /resume. `hooks[iid]` gets the PC-side actions.
    """
    session = {"id": f"{iid}-session", **({"name": name} if name else {})}
    meta = dict(
        protocol=1,
        instanceId=iid,
        pid=os.getpid(),
        cwd=cwd,
        origin=origin,
        abducoSession=abduco,
        session=session,
        startedAt=NOW - age - 60_000,
        updatedAt=NOW - age,
        status=status,
    )
    if abduco:
        # The Host reads "attached" from the terminal socket's mode; a plain listener is "not attached".
        (run / "t").mkdir(mode=0o700, exist_ok=True)
        terminal = socket.socket(socket.AF_UNIX)
        terminal.bind(str(run / "t" / abduco))
        os.chmod(run / "t" / abduco, 0o600)
        terminal.listen()
        pi.keep.append(terminal)
    instructions = {}
    clients, history, pending, queued = set(), [], list(pending), []
    seq, ids, task, partial = 1, itertools.count(1), None, ""
    model, thinking = dict(MODEL), "high"
    scoped = [dict(provider=m["provider"], id=m["id"]) for m in MODELS[:2] + MODELS[3:4]]
    default = dict(model=dict(MODEL), thinking="medium")
    labels, prompt = {}, pc_prompt
    # Saved sessions by id: the current one shares `history`.
    saved = {session["id"]: dict(session=dict(session), items=history, modified=NOW - age)}

    def send(w, frame):
        w.write(json.dumps(frame).encode() + b"\n")

    def event(kind, **data):
        nonlocal seq
        seq += 1
        frame = dict(
            type="event",
            instanceId=iid,
            sessionId=session["id"],
            seq=seq,
            timestamp=int(time.time() * 1000),
            event=dict(type=kind, **data),
        )
        for w in list(clients):
            if not w.is_closing():
                send(w, frame)

    def entry(kind, text="", **more):
        item = dict(
            kind=kind,
            entryId=str(next(ids)),
            text=text,
            timestamp=int(time.time() * 1000),
        )
        item.update(more)
        if kind == "assistant":
            item.setdefault("toolCalls", [])
        history.append(item)
        if clients:
            event("message_end", item=item)
        return item

    def result(call, text, **more):
        entry(
            "tool_result",
            text,
            toolCallId=call["id"],
            toolName=call["name"],
            isError=False,
            **more,
        )

    def set_status(value):
        meta["status"] = value
        event("status", status=value)

    async def tool(call, text, seconds=0.3, partial_text=None, **more):
        event(
            "tool_start",
            toolCallId=call["id"],
            toolName=call["name"],
            args=call["args"],
        )
        if partial_text:
            event(
                "tool_update",
                toolCallId=call["id"],
                toolName=call["name"],
                partial=dict(content=[dict(type="text", text=partial_text)]),
            )
        await asyncio.sleep(seconds)
        event(
            "tool_end",
            toolCallId=call["id"],
            toolName=call["name"],
            isError=False,
            durationMs=int(seconds * 1000),
        )
        result(call, text, **more)

    def calls(prefix):
        return [
            dict(id=f"{prefix}-read", name="read", args=dict(path="src/greet.ts")),
            dict(id=f"{prefix}-edit", name="edit", args=dict(path="src/greet.ts")),
            dict(
                id=f"{prefix}-write",
                name="write",
                args=dict(path="src/greet.test.ts", content="test\n" * 18),
            ),
            dict(id=f"{prefix}-bash", name="bash", args=dict(command="npm test")),
        ]

    async def work(prefix, bash_seconds):
        """Thinking and four tool calls, as in docs/design/html/Main.html."""
        read, edit, write, bash = calls(prefix)
        for chunk in ("greet.ts の引数を確認する。", "テストは vitest に合わせる。"):
            event("thinking_delta", delta=chunk)
            await asyncio.sleep(0.3)
        entry(
            "assistant",
            thinking="greet.ts の引数を確認する。テストは vitest に合わせる。",
            toolCalls=[read, edit, write, bash],
            stopReason="toolUse",
        )
        await tool(read, "line\n" * 42)
        await tool(edit, "ok", details=dict(patch=PATCH))
        await tool(
            write,
            "ok",
            details=dict(
                pimate=dict(
                    kind="write_diff",
                    existed=False,
                    patch="@@ -0,0 +1,18 @@\n" + "+test\n" * 18,
                )
            ),
        )
        await tool(
            bash,
            "> vitest run\n ✓ src/greet.test.ts (3 tests)\n ✓ src/cli.test.ts (2 tests)\n",
            bash_seconds,
            "> vitest run\n ✓ src/greet.test.ts (3 tests)\n ⠹ src/cli.test.ts",
        )

    async def stream(reply):
        nonlocal partial
        event("agent_start")
        set_status("running")
        aborted = False
        try:
            if reply:
                await work(
                    f"t{next(ids)}", 4
                )  # Time to steer or stop the reply from a phone.
            for i in range(0, len(BODY), 40):
                await asyncio.sleep(0.2)
                chunk = BODY[i : i + 40]
                partial += chunk
                event("text_delta", delta=chunk)
            for text in queued:
                entry("user", text)
            entry(
                "assistant",
                partial,
                model=MODEL,
                usage=dict(input=6200, output=210, cost=0.04),
                stopReason="stop",
            )
            if reply:
                pending[:] = [
                    dict(
                        kind="confirm",
                        requestId="question",
                        title="続けますか？",
                        message="作業ディレクトリの差分を確認します。",
                    )
                ]
                event("ui_request", request=pending[0])
        except asyncio.CancelledError:
            aborted = True
        finally:
            queued.clear()
            partial = ""
            event("agent_settled", aborted=aborted)
            set_status("waiting_input" if pending else "idle")

    async def hold():
        """`main`: a turn that stays at work until it is stopped."""
        event("agent_start")
        bash = calls("m")[3]
        event("tool_start", toolCallId=bash["id"], toolName="bash", args=bash["args"])
        try:
            # A running tool keeps printing, so a phone that opens later also has output to show.
            for frame in itertools.cycle("⠹⠸⠼⠴⠦⠧⠇⠏"):
                event(
                    "tool_update",
                    toolCallId=bash["id"],
                    toolName="bash",
                    partial=dict(
                        content=[
                            dict(
                                type="text",
                                text=f"> vitest run\n ✓ src/greet.test.ts (3 tests)\n {frame} src/cli.test.ts",
                            )
                        ]
                    ),
                )
                await asyncio.sleep(2)
        except asyncio.CancelledError:
            queued.clear()
            event("agent_settled", aborted=True)
            set_status("idle")

    for kind, text, more in seed:
        entry(kind, text, **more)
    for n, (title, items) in enumerate(other):
        sid = f"{iid}-saved-{n}"
        rows = []
        for kind, text, more in items:
            row = dict(kind=kind, entryId=f"s{n}-{len(rows)}", text=text, timestamp=NOW - 26 * HOUR)
            row.update(more)
            if kind == "assistant":
                row.setdefault("toolCalls", [])
            rows.append(row)
        saved[sid] = dict(
            session=dict(id=sid, name=title), items=rows, modified=NOW - (26 + n) * HOUR
        )

    def write_meta():
        (run / f"{iid}.json").write_text(json.dumps(meta))

    def switch(sid, reason):
        """Moves this Pi to another session, as /new, /resume and /fork do in the TUI."""
        saved[session["id"]].update(items=list(history), modified=int(time.time() * 1000))
        target = saved[sid]
        session.clear()
        session.update(target["session"])
        history[:] = target["items"]
        target["items"] = history
        write_meta()
        event("session_changed", reason=reason, session=dict(session))
        event("context", context=context)

    def create(title, items):
        sid = f"{iid}-{next(ids)}-session"
        saved[sid] = dict(
            session=dict(id=sid, **({"name": title} if title else {})),
            items=list(items),
            modified=int(time.time() * 1000),
        )
        return sid

    def tree():
        nodes, parent = [], None
        for i, item in enumerate(history):
            nodes.append(
                dict(
                    id=item["entryId"],
                    parentId=parent,
                    kind=item["kind"],
                    preview=(item.get("text") or item.get("toolName") or "")[:120],
                    onBranch=True,
                    isCurrent=i == len(history) - 1,
                    isLeaf=i == len(history) - 1,
                    **({"timestamp": item["timestamp"]} if "timestamp" in item else {}),
                    **({"label": labels[item["entryId"]]} if item["entryId"] in labels else {}),
                )
            )
            if i == 0:
                # A side branch left earlier, so that the tree has a fork to show.
                nodes += [
                    dict(id="side-user", parentId=item["entryId"], kind="user", preview="別の案: 設定ファイルで切り替える", onBranch=False, isCurrent=False, isLeaf=False, **({"label": labels["side-user"]} if "side-user" in labels else {})),
                    dict(id="side-answer", parentId="side-user", kind="assistant", preview="設定ファイルの案をまとめました。", onBranch=False, isCurrent=False, isLeaf=True),
                ]
            parent = item["entryId"]
        return dict(nodes=nodes, leafId=parent, truncated=False)

    async def session_op(op, arg):
        await asyncio.sleep(0.4)
        text = None
        if op == "new":
            switch(create(None, []), "new")
        elif op == "switch":
            sid = next((k for k, v in saved.items() if f"/sessions/{k}.jsonl" == arg), None)
            if sid is None:
                event("session_op_done", op=op, cancelled=True, error="セッションが見つかりません")
                return
            switch(sid, "resume")
        elif op == "fork":
            at = next((i for i, x in enumerate(history) if x["entryId"] == arg), len(history))
            switch(create(None, history[:at]), "fork")
        elif op == "tree":
            at = next((i for i, x in enumerate(history) if x["entryId"] == arg), None)
            if at is not None and history[at]["kind"] == "user":
                text = history[at]["text"]
                del history[at:]
            elif arg == "side-user":
                text = "別の案: 設定ファイルで切り替える"
                del history[1:]
            event("session_changed", reason="tree", session=dict(session))
        event(
            "session_op_done",
            op=op,
            cancelled=False,
            **({"editorText": text} if text is not None else {}),
        )

    async def compact():
        await asyncio.sleep(1.5)
        if context:
            context.update(tokens=21_000, percent=10.5)
        event("compact_done")
        event("context", context=context)

    def pc_resume():
        """Someone runs /resume in the PC terminal (8 seconds after `edge` is first opened, and on SIGUSR1)."""
        others = [k for k in saved if k != session["id"]]
        if others:
            switch(others[0], "resume")

    async def later(seconds, action):
        await asyncio.sleep(seconds)
        action()

    def pc_done():
        """SIGUSR2: the PC-only screen is closed on the PC."""
        nonlocal prompt
        if prompt:
            event("ui_prompt", phase="end", kind=prompt["kind"])
            prompt = None
            set_status("idle")

    if hooks is not None:
        hooks[iid] = (pc_resume, pc_done)

    async def rc(reader, w):
        nonlocal task, model, thinking, scoped, default
        clients.add(w)
        try:
            while line := await reader.readline():
                r = json.loads(line)
                method, params = r["method"], r.get("params") or {}
                reply, error = {}, None
                if method == "hello":
                    reply = dict(
                        snapshot=dict(
                            meta,
                            model=model,
                            thinking=thinking,
                            seq=seq,
                            pendingUI=pending,
                            streaming=dict(text=partial, thinking=""),
                            promptImages=True,
                            promptImageLimit=4,
                            entryImages=True,
                            fileImages=True,
                            slashCommands=True,
                            sessionTree=True,
                            context=context,
                            quotas=list(quotas),
                            **({"pcPrompt": prompt} if prompt else {}),
                            **(
                                dict(subagents=dict(runs=list(runs), backends=[]))
                                if runs
                                else {}
                            ),
                        )
                    )
                elif method == "history":
                    reply = dict(session=session, items=history)
                    if task is None and iid == "demo":
                        task = asyncio.create_task(stream(False))
                    elif task is None and iid == "main":
                        task = asyncio.create_task(hold())
                    elif task is None and iid == "edge":
                        task = asyncio.create_task(later(8, pc_resume))
                elif method == "prompt":
                    images = params.get("images") or []
                    more = (
                        dict(images=images, imageCount=len(images)) if images else {}
                    )
                    if task and not task.done():
                        queued.append(params["text"])
                        reply = dict(deliveredAs="steer")
                    else:
                        entry("user", params["text"], **more)
                        reply = dict(deliveredAs="prompt")
                        task = asyncio.create_task(stream(True))
                elif method == "abort" and task:
                    task.cancel()
                elif method == "ui_respond":
                    pending.clear()
                    event(
                        "ui_request_end",
                        requestId=params["requestId"],
                        by="remote",
                    )
                    set_status("idle")
                elif method == "subagent_transcript":
                    key = (params["runId"], params["childId"])
                    items = [dict(kind="assistant", entryId="analysis", text="期限切れ判定とトークン更新の呼び出しを確認しました。", toolCalls=[])] + instructions.get(key, [])
                    reply = dict(task="認証の期限切れ判定と、エラー時の処理を調べてください。", items=items[int(params.get("cursor", "0")):], cursor=str(len(items)))
                elif method == "subagent_control":
                    run_info = next(r for r in runs if r["id"] == params["runId"])
                    cid = params.get("childId")
                    if params.get("text"):
                        records = instructions.setdefault((run_info["id"], cid), [])
                        records.append(dict(kind="user", entryId=f"hint-{len(records)}", text=params["text"]))
                    else:
                        for unit in run_info["children"]:
                            if cid is None or unit["id"] == cid:
                                unit.update(state="paused", actions=[])
                        if cid is None:
                            run_info.update(state="paused", actions=[])
                    event("subagent_run", run=run_info)
                elif method == "commands":
                    reply = dict(commands=COMMANDS)
                elif method == "models":
                    reply = dict(models=MODELS, current=model, scoped=scoped, default=default)
                elif method == "set_model":
                    model = dict(provider=params["provider"], id=params["id"])
                    reply = dict(model=model)
                    event("model", model=model)
                elif method == "set_thinking":
                    thinking = params["level"]
                    reply = dict(thinking=thinking)
                    event("thinking", level=thinking)
                elif method == "set_scoped_models":
                    scoped = params["models"]
                    reply = dict(scoped=scoped)
                elif method == "set_default_model":
                    default = dict(model=params["model"], thinking=params.get("thinking"))
                    reply = dict(default=default)
                elif method == "sessions":
                    reply = dict(
                        sessions=sorted(
                            (
                                dict(
                                    id=k,
                                    path=f"/sessions/{k}.jsonl",
                                    firstMessage=next(
                                        (x["text"] for x in v["items"] if x["kind"] == "user"),
                                        "",
                                    ),
                                    messageCount=sum(
                                        x["kind"] in ("user", "assistant") for x in v["items"]
                                    ),
                                    created=v["modified"] - HOUR,
                                    modified=v["modified"],
                                    current=k == session["id"],
                                    **(
                                        {"name": v["session"]["name"]}
                                        if "name" in v["session"]
                                        else {}
                                    ),
                                )
                                for k, v in saved.items()
                            ),
                            key=lambda x: -x["modified"],
                        )
                    )
                elif method == "session_op":
                    asyncio.create_task(session_op(params["op"], params.get("arg")))
                elif method == "session_tree":
                    reply = tree()
                elif method == "set_label":
                    if params.get("label"):
                        labels[params["entryId"]] = params["label"]
                    else:
                        labels.pop(params["entryId"], None)
                elif method == "compact":
                    asyncio.create_task(compact())
                elif method == "entry_image":
                    reply = dict(image=picture(params["index"], params.get("thumbnail")))
                elif method == "file_image":
                    if params["path"].endswith("missing.png"):
                        error = dict(code="not_found", message="No such file")
                    else:
                        reply = dict(image=picture(len(params["path"]), params.get("thumbnail")))
                elif method == "shutdown":
                    send(w, dict(type="response", id=r["id"], ok=True, result={}))
                    await w.drain()
                    for path in (run / f"{iid}.json", run / f"{iid}.sock"):
                        path.unlink(missing_ok=True)
                    server.close()
                    for c in list(clients):
                        c.close()
                    return
                send(
                    w,
                    dict(type="response", id=r["id"], ok=False, error=error)
                    if error
                    else dict(type="response", id=r["id"], ok=True, result=reply),
                )
                await w.drain()
        finally:
            clients.discard(w)
            w.close()

    server = await asyncio.start_unix_server(rc, run / f"{iid}.sock", limit=1 << 20)  # 画像つきの prompt は 1 行が大きい
    write_meta()
    return server


pi.keep = []


def child(index, agent, title):
    return dict(
        id=str(index),
        index=index,
        agent=agent,
        title=title,
        state="running",
        hasTranscript=True,
        actions=["steer", "follow_up", "stop_child"],
    )


async def demo(lab, binary):
    run = lab / "run"
    run.mkdir(mode=0o700)
    env = dict(
        os.environ,
        PIMATE_CONFIG_DIR=str(lab / "config"),
        PIMATE_RUN_DIR=str(run),
        HOME=str(lab),
        PIMATE_PI="/bin/false",
        XDG_CONFIG_HOME=str(lab / "xdg"),
        PI_CODING_AGENT_DIR=str(lab / "agent"),
    )
    read, edit, write, bash = (
        dict(id=f"m-{n}", name=n, args=a)
        for n, a in [
            ("read", dict(path="src/greet.ts")),
            ("edit", dict(path="src/greet.ts")),
            ("write", dict(path="src/greet.test.ts")),
            ("bash", dict(command="npm test")),
        ]
    )
    old = [
        dict(id=f"m0-{i}", name=n, args=a)
        for i, (n, a) in enumerate(
            [
                ("ls", dict(path="src")),
                ("read", dict(path="package.json")),
                ("read", dict(path="src/cli.ts")),
                ("read", dict(path="src/greet.ts")),
            ]
        )
    ]

    def done(call, text, **more):
        return (
            "tool_result",
            text,
            dict(toolCallId=call["id"], toolName=call["name"], isError=False, **more),
        )

    hooks = {}
    context = dict(tokens=82_000, contextWindow=200_000, percent=41)
    # Sheet-Usage: a window whose reset has passed, a weekly one, and one that is used up.
    quotas = [
        dict(
            source="claude",
            observedAt=NOW - 49 * 60_000,
            windows=[
                dict(minutes=300, usedPercent=38, resetsAt=NOW - 20 * 60_000),
                dict(minutes=10080, usedPercent=67, resetsAt=NOW + 71 * HOUR),
            ],
        ),
        dict(
            source="codex",
            observedAt=NOW - 5 * 60_000,
            windows=[
                dict(minutes=300, usedPercent=12, resetsAt=NOW + 3 * HOUR),
                dict(minutes=10080, usedPercent=100, resetsAt=NOW + 5 * 24 * HOUR),
            ],
        ),
    ]
    servers = [
        await pi(
            run,
            "demo",
            "/demo/android",
            context=dict(tokens=None, contextWindow=200_000, percent=None),
        ),
        await pi(
            run,
            "main",
            "/home/me/code/demo",
            name="--shout フラグを追加",
            status="running",
            origin="mobile",
            abduco="pimate-de7f3a00",
            context=dict(context),
            quotas=quotas,
            other=[
                (
                    "API クライアントのリファクタ",
                    [
                        ("user", "API クライアントを fetch に置き換えて", {}),
                        ("assistant", "置き換えました。", dict(stopReason="stop")),
                    ],
                ),
                (None, [("user", "README の誤字を直して", {})]),
            ],
            seed=[
                ("user", "プロジェクトの構成を説明して", dict(timestamp=NOW - 78_000)),
                ("assistant", "", dict(toolCalls=old, stopReason="toolUse")),
                *(done(c, "line\n" * 12) for c in old),
                (
                    "assistant",
                    "CLI のエントリは `src/cli.ts`、挨拶の生成は `src/greet.ts` にまとまっています。",
                    dict(
                        model=MODEL,
                        usage=dict(input=6200, output=210, cost=0.04),
                        stopReason="stop",
                        timestamp=NOW - 60_000,
                    ),
                ),
                (
                    "user",
                    "@src/greet.ts に --shout フラグを追加して。テストも書いて",
                    {},
                ),
                (
                    "assistant",
                    "",
                    dict(
                        thinking="greet.ts の引数を確認する。テストは vitest に合わせる。",
                        toolCalls=[read, edit, write, bash],
                        stopReason="toolUse",
                    ),
                ),
                done(read, "line\n" * 42),
                done(edit, "ok", details=dict(patch=PATCH)),
                done(
                    write,
                    "ok",
                    details=dict(
                        pimate=dict(
                            kind="write_diff",
                            existed=False,
                            patch="@@ -0,0 +1,18 @@\n" + "+test\n" * 18,
                        )
                    ),
                ),
            ],
        ),
        await pi(
            run,
            "waiting",
            "/home/me/work/login",
            name="ログイン画面の改修",
            status="waiting_input",
            context=dict(context),
            # Chat-Waiting: the rings show the shortest window (67% and a used-up 100%).
            quotas=[
                dict(source=q["source"], observedAt=q["observedAt"], windows=[dict(minutes=300, usedPercent=used, resetsAt=NOW + 3 * HOUR)])
                for q, used in zip(quotas, (67, 100))
            ],
            seed=[
                (
                    "user",
                    "ログイン画面をメールリンク認証に対応させて",
                    dict(timestamp=NOW - 52_000),
                ),
                ("assistant", "", dict(toolCalls=old[:2], stopReason="toolUse")),
                *(done(c, "line\n" * 12) for c in old[:2]),
                (
                    "assistant",
                    "現状の構成を確認しました。\n\n- `LoginForm.tsx` はパスワード入力のみ\n- 認証は `/api/session` に集約\n\n実装方針を選んでください。",
                    dict(stopReason="stop"),
                ),
            ],
            pending=[
                dict(
                    kind="questionnaire",
                    requestId="method",
                    title="メールリンクの実装方式は？",
                    questions=[
                        dict(
                            question="メールリンクの実装方式は？",
                            options=[
                                dict(
                                    label="既存の認証 API を拡張",
                                    description="変更は 3 ファイル程度",
                                ),
                                dict(
                                    label="外部の Magic Link サービスを使う",
                                    description="SDK の追加と環境変数が必要",
                                ),
                                dict(
                                    label="今回は UI の整理だけ",
                                    description="認証方式は変更しない",
                                ),
                            ],
                        )
                    ],
                )
            ],
        ),
        await pi(
            run,
            "api",
            "/home/me/work/api",
            name="認証まわりのレビュー",
            seed=[("user", "認証を並列にレビューしてください。", {}), ("assistant", "レビュアーをバックグラウンドで起動しました。", dict(stopReason="stop"))],
            age=5 * 60_000,
            runs=[
                dict(
                    id="run",
                    backend="pi-subagents",
                    background=True,
                    mode="parallel",
                    state="running",
                    children=[
                        child(0, "reviewer", "認証"),
                        child(1, "reviewer", "セッション"),
                    ],
                    actions=["stop"],
                ),
                dict(id="scout", backend="example", background=True, mode="chain", state="complete", stage=dict(index=0,count=3), children=[dict(child(0,"scout","ログイン処理の調査"), state="complete", actions=[])], actions=[]),
            ],
        ),
        await pi(
            run,
            "website",
            "/home/me/code/website",
            name="README の整理",
            origin="mobile",
            age=60 * 60_000,
        ),
        # Chat-Edge: switched to the saved session 8 seconds after it is first opened (and on SIGUSR1),
        # as /resume on the PC does.
        await pi(
            run,
            "edge",
            "/home/me/work/errors",
            name="API クライアントのリファクタ",
            age=10 * 60_000,
            context=dict(context),
            hooks=hooks,
            seed=[
                ("user", "タイムアウトしたときの再試行をまとめて", dict(timestamp=NOW - 700_000)),
                ("assistant", "", dict(toolCalls=old, stopReason="toolUse")),
                *(done(c, "line\n" * 12) for c in old),
                (
                    "assistant",
                    "リトライ処理を `withRetry()` に共通化しました。タイムアウト時は最大 3 回まで再試行します。",
                    dict(
                        model=dict(provider="anthropic", id="claude-sonnet-5-5"),
                        usage=dict(input=18400, output=640, cost=0.09),
                        stopReason="stop",
                        timestamp=NOW - 659_000,
                    ),
                ),
            ],
            other=[
                (
                    "エラー処理の見直し",
                    [
                        ("user", "エラーメッセージを整理して", {}),
                        (
                            "assistant",
                            "エラーメッセージを `errors.ts` にまとめ、文言を日本語にそろえました。",
                            dict(
                                model=dict(provider="anthropic", id="claude-sonnet-5-5"),
                                usage=dict(input=9100, output=380, cost=0.04),
                                stopReason="stop",
                            ),
                        ),
                    ],
                )
            ],
        ),
        # Chat-Attach: an extension screen that only the PC terminal can operate. SIGUSR2 closes it.
        await pi(
            run,
            "attach",
            "/home/me/work/ops",
            name="ブランチの整理",
            status="waiting_input",
            abduco="pimate-0091c2ab",
            origin="mobile",
            age=2 * 60_000,
            context=dict(context),
            pc_prompt=dict(kind="custom", title="git-graph"),
            hooks=hooks,
            seed=[
                ("user", "ローカルのブランチを一覧にして", dict(timestamp=NOW - 200_000)),
                ("assistant", "", dict(toolCalls=old[:1], stopReason="toolUse")),
                done(old[0], "main\n"),
                (
                    "assistant",
                    "ブランチは 14 本あり、そのうち 6 本は `main` にマージ済みです。",
                    dict(
                        model=dict(provider="anthropic", id="claude-sonnet-5-5"),
                        usage=dict(input=4800, output=160, cost=0.02),
                        stopReason="stop",
                        timestamp=NOW - 194_000,
                    ),
                ),
                ("user", "コミット履歴を見ながら、不要なブランチを整理して", dict(timestamp=NOW - 120_000)),
                ("assistant", "", dict(toolCalls=old[1:3], stopReason="toolUse")),
                *(done(c, "line\n" * 12) for c in old[1:3]),
                (
                    "assistant",
                    "グラフを開きました。削除するブランチを選んでください。",
                    dict(stopReason="stop", timestamp=NOW - 111_000),
                ),
            ],
        ),
        # Pictures: sent ones, a tool result, omitted previews (+N, fetched on demand) and Markdown images.
        await pi(
            run,
            "images",
            "/home/me/work/shots",
            name="スクリーンショットの確認",
            age=3 * 60_000,
            context=dict(context),
            seed=[
                (
                    "user",
                    "この 2 枚の違いを見て",
                    dict(images=[preview(0), preview(1)], imageCount=2),
                ),
                (
                    "assistant",
                    "",
                    dict(
                        toolCalls=[dict(id="shot", name="screenshot", args=dict(path="out/login.png"))],
                        stopReason="toolUse",
                    ),
                ),
                (
                    "tool_result",
                    "1 枚を保存しました",
                    dict(toolCallId="shot", toolName="screenshot", isError=False, images=[preview(2)], imageCount=1),
                ),
                (
                    "assistant",
                    "ボタンの位置が 8px ずれています。\n\n### ![修正前の画面](out/before.png)\n\n修正後は次のとおりです。\n\n![修正後の画面](out/after.png)\n\n| 確認箇所 | プレビュー |\n| --- | --- |\n| ログイン | ![確認結果](out/result.png) |\n\n読めない画像: ![見つからない画像](out/missing.png)",
                    dict(stopReason="stop"),
                ),
                (
                    "user",
                    "残りの 5 枚も確認して",
                    dict(images=[preview(0), preview(1)], imageIndices=[0, 1], imageCount=5),
                ),
                ("assistant", "5 枚とも問題ありません。", dict(stopReason="stop")),
            ],
        ),
    ]
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    if port == 47800:
        raise RuntimeError("Reserved TS Host port; restart the demo")
    if name := os.environ.get("PIMATE_DEMO_NAME"):
        import uuid
        (lab / "config").mkdir(mode=0o700)
        (lab / "config/host.json").write_text(json.dumps(dict(name=name, hostId=str(uuid.uuid4()))))
    args = ["--port", str(port)]
    host = await asyncio.create_subprocess_exec(
        binary, "serve", *args, "--listen", "127.0.0.1", env=env
    )
    fixture = None
    try:
        for _ in range(100):
            if host.returncode is not None:
                raise RuntimeError("Host failed")
            try:
                _, writer = await asyncio.open_connection("127.0.0.1", port)
                writer.close()
                break
            except OSError:
                await asyncio.sleep(0.05)
        else:
            raise RuntimeError("Host timeout")
        # The PC-side actions of the mocks: `kill -USR1 <this pid>` runs /resume on `edge`,
        # `kill -USR2` closes the PC-only screen of `attach`.
        loop = asyncio.get_running_loop()
        loop.add_signal_handler(signal.SIGUSR1, hooks["edge"][0])
        loop.add_signal_handler(signal.SIGUSR2, hooks["attach"][1])
        print(f"pid {os.getpid()}（kill -USR1: edge で PC の /resume、kill -USR2: attach の PC の画面を閉じる）", flush=True)
        if mode := os.environ.get("PIMATE_DEMO_SCREENS"):
            if mode not in {"list", "empty", "reconnect"}:
                raise ValueError("PIMATE_DEMO_SCREENS must be list, empty or reconnect")
            fixture = await asyncio.create_subprocess_exec(
                "node", ROOT / "scripts/android-screen-fixture.cjs", str(port), mode,
                stdout=asyncio.subprocess.PIPE,
            )
            port = int(await asyncio.wait_for(fixture.stdout.readline(), timeout=5))
        print(f"adb -s SERIAL reverse tcp:{port} tcp:{port}", flush=True)
        subprocess.run(
            [binary, "pair", *args, "--address", f"127.0.0.1:{port}"],
            env=env,
            check=True,
        )
        await host.wait()
    finally:
        if fixture is not None and fixture.returncode is None:
            fixture.terminate()
            await fixture.wait()
        if host.returncode is None:
            host.terminate()
            await host.wait()
        for server in servers:
            server.close()
            await server.wait_closed()


if __name__ == "__main__":
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "pimate-host"], cwd=ROOT, check=True
    )
    os.chdir(ROOT)
    signal.signal(signal.SIGTERM, signal.default_int_handler)
    with tempfile.TemporaryDirectory(prefix="pimate-demo-", dir="/tmp") as tmp:
        try:
            asyncio.run(
                demo(
                    Path(tmp),
                    Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
                    / "debug/pimate",
                )
            )
        except KeyboardInterrupt:
            pass
