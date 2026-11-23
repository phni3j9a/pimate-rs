# pimate-rs

[PiMate](https://github.com/phni3j9a/PiMate)（PC で動く [Pi Coding Agent](https://github.com/earendil-works/pi) をスマホなどから使うツール）の Rust 版です。最終的に TS 版を置き換えます。

**状態:** protocol v1 と Host 基盤（発見・ペアリング・認証・中継）を実装しました。Host の起動管理とアプリは今後の範囲です（[ロードマップ #4](https://github.com/phni3j9a/pimate-rs/issues/4)）。

## 目的

- iOS とデスクトップを含めて、アプリを同じ作りにまとめる
- Host を単一バイナリにして、Node や abduco への依存を減らす

## 構成

| 部分 | 作り |
|---|---|
| 中核（protocol・通信・状態） | Rust。すべての OS で共有する |
| iOS アプリ | SwiftUI。UniFFI で中核とつなぐ |
| Android アプリ | Jetpack Compose。UniFFI で中核とつなぐ |
| macOS / Linux アプリ | GPUI（Rust）。中核を直接呼ぶ |
| Host（PC で常駐） | Rust の単一バイナリ |
| Remote Control Extension | TS のまま。Pi のプロセス内で動くため。protocol の TS の型は Rust から生成する |

Discord の新しいクライアント（Phoenix）と同じ形です。選んだ理由と調査の結果は [docs/research/ui-frameworks.md](docs/research/ui-frameworks.md) にあります。

## 開発

ルートは Cargo workspace（edition 2024）です。[`crates/pimate-protocol`](crates/pimate-protocol) にアプリ ⇔ Host（WebSocket）・Host ⇔ Extension（NDJSON）の wire 型と検査・変換の関数があります。

```sh
cargo test
cargo run --quiet --example bindings > bindings/ts/index.ts  # Extension 用の型を再生成
```

[`bindings/ts/index.ts`](bindings/ts/index.ts) はコミット対象です。CI は生成し直して差分を確かめます。TS 版との照合には Node 22.18 以降と、依存を導入した PiMate のチェックアウトが必要です。`scripts/check-upstream.sh /path/to/PiMate` で型の相互代入と fixture の一致を検査できます。`--write-fixtures` を末尾に付けると、[TS スクリプト](scripts/fixtures.ts) の出力で [`tests/fixtures/wire.json`](crates/pimate-protocol/tests/fixtures/wire.json) を更新します。元にした PiMate は `c5307f4b5f927926afddde2123501a49b7ec71e8` です。

Extension / Host の JSON フレームは `decode_json`、NDJSON は `LineDecoder` で読みます。TS が UTF-16 の途中で表示テキストを切った場合、孤立したサロゲートだけは Rust の文字列に入らないので `�` に置き換えます。

**Host の中継とアプリの読み込みは分けます。** Host は RC の要求・snapshot・history・イベント・応答を `serde_json::Value` のまま保持します。`instanceId`、`method`、セッションや連番など自分の判断に必要な部分だけを検査し、転送する内容を `InstanceSnapshot`・`InstanceEvent`・`ServerMessage` などから作り直しません。イベントは元のフレームのまま、`subscribed` と `rc_result` は `relay::HostFrame` で元の JSON を包んで送ります。`AppMessage::Rc` 内の `Request` は中継用の生の JSON オブジェクトです。Extension が実行するときは、追加キーマップのない `RequestOf<M>` と値の検査を使います。

アプリは `HostMessage` などの型で読み、未知の操作・バックエンドは一覧の該当要素だけ、未知バックエンドの `subagent_run` はそのイベントだけを読み飛ばします。既知バックエンドの不正な run はエラーです。

`host_restart`・`github_status`・端末管理・更新・GitHub 要求などの余分なキーを拒否する検査は、Host が生の JSON を受け取った時点で行います。型への変換は、認証・権限・要求内容の検査の代わりにはなりません。要求は自動再送しません。

## Host を試す（Linux / macOS）

[`crates/pimate-host`](crates/pimate-host) は `pimate` 単一バイナリです。既存 Host と並べて試す場合、**両方の端末で同じ一時ディレクトリと別ポート**を指定してください。`list` / `serve` は runDir の古い登録を掃除します。

```sh
cargo build -p pimate-host
export PATH="$PWD/target/debug:$PATH"
lab=$(mktemp -d); export PIMATE_RUN_DIR="$lab/run" PIMATE_CONFIG_DIR="$lab/config"
pimate serve --port 47801 & host_pid=$!
pimate pair --port 47801          # URL と QR（10分・1回限り）
pimate devices                   # token / hash は表示しない
# pimate revoke <deviceId>
pimate list                      # 同じ runDir で TS Extension をロードした Pi の一覧
kill "$host_pid"                  # この例で起動した Host のみ停止
```

待ち受けは `127.0.0.1` と、`tailscale ip` が報告しローカル NIC に存在する Tailscale IP のみです。`--listen` はこの集合を絞り込みます。TLS は内蔵せず、端末からの接続には Tailscale とその ACL を使います。`PIMATE_CONFIG_DIR` と `PIMATE_RUN_DIR` の既定値・0700 の制約・0600 のファイル形式は TS 版と共通です。`devices.lock` 競合時は操作をやり直してください。強制終了で残ったロックを削除するのは、Host / CLI の書き手がすべて停止したと確認した後だけです。

通常の `cargo test` は一時ディレクトリ・ループバックの自動割当ポートで模擬 RC を使います。実物の Pi 1.6.0 と依存導入済みの TS 版 PiMate があれば、下記で PTY 上の Pi を起動し、発見・購読・prompt・faux モデルの返答まで検査できます。ネットワークの LLM・既存 Host・既存 Pi の設定は使いません。

```sh
PIMATE_TEST_PI="$HOME/.pi/agent/bin/pi" PIMATE_TEST_TS=/path/to/PiMate \
  cargo test -p pimate-host --test host real_pi -- --ignored
```

## 決まったこと

- アプリは iOS / Android / Linux / macOS の4つの OS に対応する
- WebView を使う方式（Dioxus の WebView 描画、Tauri など）は、アプリの質が下がるので使わない
- Linux で GPUI の日本語入力が通らなければ、Linux だけ GTK4（relm4）にする（[#2](https://github.com/phni3j9a/pimate-rs/issues/2) で確かめる）
- Host は端末の保持（今の abduco の役割）も内蔵する。段階を踏んで進める
  1. protocol を Rust の crate にする（[#1](https://github.com/phni3j9a/pimate-rs/issues/1)）
  2. Host を Rust に移植する（端末の保持は、いったん abduco を使う）
  3. 端末の保持を自前の実装に置き換える
- Host のバイナリは OS と CPU ごとに用意し、Pi のパッケージ（npm）に同梱する。利用者が Rust を入れる必要はない
- 通知は当面作らない
- iOS / macOS のビルドと確認は Devin Cloud の macOS VM で行う（手順は TS 版の [docs/devin-macos.md](https://github.com/phni3j9a/PiMate/blob/main/docs/devin-macos.md)）

## ライセンス

[MIT License](LICENSE)
