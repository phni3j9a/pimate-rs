#!/usr/bin/env python3
"""A disposable, loopback-only Rust Host plus in-memory NDJSON RCs.

Instances mirror the design mocks (docs/design): `demo` streams Markdown and takes prompts,
`main` stays at work with a running tool, `waiting` holds a question, the rest fill the list.
"""

import asyncio
import itertools
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time

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
):
    """One fake Pi: `<iid>.sock` + `<iid>.json` in the Host's run dir."""
    session = {"id": f"{iid}-session", **({"name": name} if name else {})}
    meta = dict(
        protocol=1,
        instanceId=iid,
        pid=os.getpid(),
        cwd=cwd,
        origin=origin,
        abducoSession=None,
        session=session,
        startedAt=NOW - age - 60_000,
        updatedAt=NOW - age,
        status=status,
    )
    clients, history, pending, queued = set(), [], list(pending), []
    seq, ids, task, partial = 1, itertools.count(1), None, ""

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
        event(
            "tool_update",
            toolCallId=bash["id"],
            toolName="bash",
            partial=dict(
                content=[
                    dict(
                        type="text",
                        text="> vitest run\n ✓ src/greet.test.ts (3 tests)\n ⠹ src/cli.test.ts",
                    )
                ]
            ),
        )
        try:
            await asyncio.Event().wait()
        except asyncio.CancelledError:
            queued.clear()
            event("agent_settled", aborted=True)
            set_status("idle")

    for kind, text, more in seed:
        entry(kind, text, **more)

    async def rc(reader, w):
        nonlocal task
        clients.add(w)
        try:
            while line := await reader.readline():
                r = json.loads(line)
                method = r["method"]
                reply = {}
                if method == "hello":
                    reply = dict(
                        snapshot=dict(
                            meta,
                            model=MODEL,
                            thinking="high",
                            seq=seq,
                            pendingUI=pending,
                            streaming=dict(text=partial, thinking=""),
                            **(
                                dict(subagents=dict(runs=list(runs), backends=[]))
                                if runs
                                else {}
                            ),
                        )
                    )
                elif method == "history":
                    reply = dict(session=session, items=history)
                    if task is None and iid == "demo" and not r["id"].startswith("drawer-"):
                        task = asyncio.create_task(stream(False))
                    elif task is None and iid == "main" and not r["id"].startswith("drawer-"):
                        task = asyncio.create_task(hold())
                elif method == "prompt":
                    if task and not task.done():
                        queued.append(r["params"]["text"])
                        reply = dict(deliveredAs="steer")
                    else:
                        entry("user", r["params"]["text"])
                        reply = dict(deliveredAs="prompt")
                        task = asyncio.create_task(stream(True))
                elif method == "abort" and task:
                    task.cancel()
                elif method == "ui_respond":
                    pending.clear()
                    event(
                        "ui_request_end",
                        requestId=r["params"]["requestId"],
                        by="remote",
                    )
                    set_status("idle")
                send(w, dict(type="response", id=r["id"], ok=True, result=reply))
                await w.drain()
        finally:
            clients.discard(w)
            w.close()

    server = await asyncio.start_unix_server(rc, run / f"{iid}.sock")
    (run / f"{iid}.json").write_text(json.dumps(meta))
    return server


def child(index, agent, title):
    return dict(
        id=str(index),
        index=index,
        agent=agent,
        title=title,
        state="running",
        hasTranscript=False,
        actions=[],
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
        PIMATE_ABDUCO="/bin/false",
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

    servers = [
        await pi(run, "demo", "/demo/android"),
        await pi(
            run,
            "main",
            "/home/me/code/demo",
            name="--shout フラグを追加",
            status="running",
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
                    actions=[],
                )
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
        print(f"adb -s SERIAL reverse tcp:{port} tcp:{port}", flush=True)
        subprocess.run(
            [binary, "pair", *args, "--address", f"127.0.0.1:{port}"],
            env=env,
            check=True,
        )
        await host.wait()
    finally:
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
