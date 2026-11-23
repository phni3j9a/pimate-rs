#!/usr/bin/env python3
"""A disposable, loopback-only Rust Host plus an in-memory NDJSON RC."""

import asyncio
import itertools
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BODY = (
    "## 開発用の会話\n\n| 項目 | 状態 |\n| --- | --- |\n| Android | 接続済み |\n\n"
    '```kotlin\nval greeting = "こんにちは"\n```\n\n'
    + "日本語の長い文章と折り返しを確認します。" * 14
    + "\n\n[PiMate](https://github.com/phni3j9a/pimate-rs)"
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
    meta = dict(
        protocol=1,
        instanceId="demo",
        pid=os.getpid(),
        cwd="/demo/android",
        origin="pc",
        abducoSession=None,
        session={"id": "demo-session"},
        startedAt=1,
        updatedAt=1,
        status="idle",
    )
    clients, history = set(), []
    seq, ids, task, partial, pending = 1, itertools.count(1), None, "", []

    def send(w, frame):
        w.write(json.dumps(frame).encode() + b"\n")

    def event(kind, **data):
        nonlocal seq
        seq += 1
        for w in list(clients):
            if not w.is_closing():
                send(
                    w,
                    dict(
                        type="event",
                        instanceId="demo",
                        sessionId="demo-session",
                        seq=seq,
                        event=dict(type=kind, **data),
                    ),
                )

    def entry(kind, text):
        item = dict(kind=kind, entryId=str(next(ids)), text=text)
        if kind == "assistant":
            item["toolCalls"] = []
        history.append(item)
        event("message_end", item=item)

    async def stream(reply):
        nonlocal partial
        event("agent_start")
        try:
            for i in range(0, len(BODY), 20):
                await asyncio.sleep(0.28)
                chunk = BODY[i : i + 20]
                partial += chunk
                event("text_delta", delta=chunk)
            entry("assistant", partial)
            if reply:
                pending[:] = [
                    dict(kind="confirm", requestId="question", title="続けますか？")
                ]
                event("ui_request", request=pending[0])
        finally:
            partial = ""
            event("agent_settled", aborted=False)

    async def rc(reader, w):
        nonlocal task
        clients.add(w)
        try:
            while line := await reader.readline():
                r = json.loads(line)
                method = r["method"]
                result = {}
                if method == "hello":
                    result = dict(
                        snapshot=dict(
                            meta,
                            model=None,
                            thinking=None,
                            seq=seq,
                            pendingUI=pending,
                            streaming=dict(text=partial, thinking=""),
                        )
                    )
                elif method == "history":
                    result = dict(session=meta["session"], items=history)
                    if task is None:
                        task = asyncio.create_task(stream(False))
                elif method == "prompt":
                    entry("user", r["params"]["text"])
                    result = dict(deliveredAs="prompt")
                    if not task or task.done():
                        task = asyncio.create_task(stream(True))
                elif method == "abort" and task:
                    task.cancel()
                elif method == "ui_respond":
                    pending.clear()
                    event("ui_request_end", requestId="question", by="remote")
                send(w, dict(type="response", id=r["id"], ok=True, result=result))
                await w.drain()
        finally:
            clients.discard(w)
            w.close()

    server = await asyncio.start_unix_server(rc, run / "demo.sock")
    (run / "demo.json").write_text(json.dumps(meta))
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    if port == 47800:
        raise RuntimeError("Reserved TS Host port; restart the demo")
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
