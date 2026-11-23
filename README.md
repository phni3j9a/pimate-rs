# pimate-rs

[PiMate](https://github.com/phni3j9a/PiMate)（PC で動く [Pi Coding Agent](https://github.com/earendil-works/pi) をスマホなどから使うツール）の Rust 版です。最終的に TS 版を置き換えます。

**状態:** protocol v1 と Host 基盤（発見・ペアリング・認証・中継）を実装しました。スマホからの Pi の起動・PC での attach・ログイン専用 Pi にも対応しました。外部エージェント用 CLI・Webhook・GitHub 閲覧・Linux の常駐登録にも対応しています。アプリと Extension の Rust Host への切替は今後の範囲です（[ロードマップ #4](https://github.com/phni3j9a/pimate-rs/issues/4)）。

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
lab=$(mktemp -d /tmp/pimate.XXXXXX); export PIMATE_RUN_DIR="$lab/run" PIMATE_CONFIG_DIR="$lab/config"
export PI_CODING_AGENT_DIR="$lab/agent" XDG_CONFIG_HOME="$lab/xdg"
# Linux x64 の開発用（他の OS / CPU では対応する abduco の絶対パス）
export PIMATE_ABDUCO="$PWD/bin/linux-x64/abduco"
export PIMATE_EXTENSION=/path/to/PiMate/packages/pimate/src/extension/index.ts
pimate serve --port 47801 & host_pid=$!
pimate pair --port 47801          # URL と QR（10分・1回限り）
pimate devices                   # token / hash は表示しない
# pimate revoke <deviceId>
pimate list                      # 同じ runDir で TS Extension をロードした Pi の一覧
pimate attach                     # 同じ runDir の Rust Host が起動した端末一覧
# pimate attach pimate-01234567   # Ctrl+\ で切り離す（Pi は動き続ける）
kill "$host_pid"                  # この例で起動した Host のみ停止
```

待ち受けは `127.0.0.1` と、`tailscale ip` が報告しローカル NIC に存在する Tailscale IP のみです。`--listen` はこの集合を絞り込みます。TLS は内蔵せず、端末からの接続には Tailscale とその ACL を使います。`PIMATE_CONFIG_DIR` と `PIMATE_RUN_DIR` の既定値・0700 の制約・0600 のファイル形式は TS 版と共通です。`devices.lock` 競合時は操作をやり直してください。強制終了で残ったロックを削除するのは、Host / CLI の書き手がすべて停止したと確認した後だけです。

通常の `cargo test` は一時ディレクトリ・ループバックの自動割当ポートで模擬 RC を使います。実物の Pi 1.6.0 と依存導入済みの TS 版 PiMate があれば、下記で PTY 上の Pi を起動し、発見・購読・prompt・faux モデルの返答まで検査できます。ネットワークの LLM・既存 Host・既存 Pi の設定は使いません。

```sh
TMPDIR=/tmp PIMATE_TEST_PI="$HOME/.pi/agent/bin/pi" PIMATE_TEST_TS=/path/to/PiMate \
  cargo test -p pimate-host --test host real_pi -- --ignored
```

### スマホからの起動とログイン

ペアリング済み WS の `folders` / `dirs` で起動先を選び、`spawn` へ `cwd` と任意の `model`・`thinking`・`askTool`・`trust`・初回 `message` を渡します。受理した起動はスマホが切断しても初回メッセージ送信まで継続します。初回メッセージの失敗で Pi や prompt を自動再試行しません。終了は既存の RC `shutdown`。Rust Host は `PIMATE_RUN_DIR/t/`（0700）を端末専用の名前空間にします。起動・一覧・attach・終了済み端末の回収はこの中だけで行い、環境の `ABDUCO_SOCKET_DIR`・`~/.abduco`・他の runDir は使いません。ソケットは `<runDir>/t/<名前>` に置き、所有記録・ホスト名・abduco の一覧には依存しません。Host はソケットのモードから接続中・終了済みを判定し、名前指定の attach はそのパスだけを見ます。一覧は接続できる端末だけを表示します。

制限: keeper が異常終了した場合、到達不能なソケットファイルを自動削除せず runDir に残します（通常の一時 runDir は再起動・ログアウトで消えます）。

`PIMATE_PI`（既定 `pi`）と TS Extension が必要です。`PIMATE_EXTENSION` はファイルパスまたは Pi のパッケージ指定（`npm:`、`git:` など）です。端末管理は `terminal.rs` に隔離し、`PIMATE_ABDUCO` → `pimate` と同じディレクトリの検証済みバイナリ（現在 Linux x64、[チェックサム](bin/SHA256SUMS.json)）→ PATH の順で解決します。配布時は `bin/<platform>-<arch>/` に並べ、Extension の既定はパッケージの `dist/extension.js` です。Linux の user systemd が使えれば Pi ごとの scope、それ以外は独立した端末セッションを使います。後者はログアウトやサービスの cgroup 終了に対する保証はありません。

`auth_session` のログイン専用 Pi は接続・`holdId` ごとに保持し、`auth_session_end` または切断で解放します。通常の一覧・購読には出さず、`hello` と認証用 RC だけを許可します。起動待ちの保持を解放・切断すると、新たなログイン Pi は起動しません。すでに始まった起動は完了させます。最後の解放から60秒経過後、保存中なら延期し、対話中ならキャンセルして終了します。

上記の `real_pi -- --ignored` は WS 起動・初回 prompt・Host 再起動・PTY attach・専用 Pi の保持と終了まで検査します（1分強）。Linux x64 以外の実 Pi 試験には `PIMATE_TEST_ABDUCO=/absolute/path/to/abduco` も指定してください。Linux x64 でも同じ指定で upstream v0.6 のバイナリを検証できます。`serve` / `attach` / `list` には同じ短い絶対パスの `PIMATE_RUN_DIR` を指定してください。端末ソケットのパス、または RC の `<runDir>/<UUID>.sock` が OS のバイト長上限に達する場合、起動はエラーになります。

### 外部エージェントと Webhook

`spawn --cwd DIR [-- pi args]` は `origin: "grok-bot"` の新しい Pi を作り、`instanceId`・`attachCommand` などを JSON で返します。`prompt ID TEXT` は `--steer` / `--follow-up` を選択でき、`TEXT` が `-` なら標準入力を読みます。`abort ID` はターンを中断、`shutdown ID` は Pi を終了します。`watch ID` は RC の生イベントを表示します。

```sh
pimate prompt "$id" 'この変更を確認して' --wait --timeout 60
pimate wait "$id" --timeout 60      # timeout は送信失敗ではない。prompt は再送しない
pimate history "$id" --last 4        # 直近の会話を古い順で返す
```

`outcome` は `done` / `failed` / `aborted` / `waiting_input` / `timeout` / `idle` / `closed`。`waiting_input` の `pending` に対して `ui ID REQUEST <値|--confirm|--cancel|--answers JSON>` で回答します。`--wait` は既定300秒。結果に含まれる `failed`・`timeout` と、CLI 自体の失敗（終了コード1）は異なります。

Webhook は任意です。`webhook setup [--machine 表示名]` で URL とヘッダー（`Authorization: ...`）を端末の標準入力から渡します。`webhook test` は実際に送信します。`webhook watch ID` で明示した Pi の質問・解決・完了・失敗・子の待機などを送ります。`status` には資格情報を出しません。解除は `webhook unwatch ID`、設定ごとの削除は `webhook off` です。秘密の伏せ字は完全ではなく、送信先には質問や回答の一部が届きます。初回送信の100枠が埋まった場合、新規通知はログに記録して破棄します。配信済みまたは配信中の質問に対する解決通知だけは、質問の配信成功後に別タスクで枠を待ち、Pi の監視を止めません。再送キューはメモリ内のみ。アプリへの push 通知は実装しません。

### 管理と常駐登録

認証済み WS の `github` は対象の Pi の作業ディレクトリに限り Issue / PR を読みます。`github_status` は github.com のログイン名だけを取得します。`update_check` は npmjs.org の最新版を確認し、`update` は公開 npm パッケージに同梱され、かつ `pimate-host.service` で稼働する場合だけ固定引数で更新します。`host_restart` はこの常駐サービスだけが対象です。比較元は起動時の npm 版（パッケージ外は crate の版・更新不可）です。更新と再起動は端末をまたいで排他し、競合時は更新に `busy`、再起動に `failed` を返します。`gh` の stderr には認証情報が含まれ得るため、TS 版とは異なり、失敗は分類コードと固定文言のみを返します。

Linux の user systemd の登録・確認・削除は `pimate service install|status|uninstall`。`start|stop|health|has-systemd` と `command-status|command-install|command-remove` も `service` のサブコマンドです。登録・削除は実際にサービス状態を変えます。**稼働中の TS Host と同じポート・サービス名で試さないでください。** `XDG_CONFIG_HOME` を変えても systemd の user manager は分離されません。Rust の管理印・設定ディレクトリと異なる unit は上書きしません。linger の設定は行いません。

Extension のために `setup-info`、`pair --json [--address host:port]`（秘密のリンクと失効時刻）、`pair-pending CODE`、`devices`、`revoke ID --json` と各 `service` 操作が JSON を返します。TS Extension の変更は M7 です。テストは管理コマンドを偽物、Webhook を loopback に切り替えます。`PIMATE_SERVICE_PORT` はサービス管理・health 確認用のポート（通常 47800）です。

TS の配布用 CLI がビルド済みなら、`PIMATE_TEST_TS_CLI=/path/to/PiMate/packages/pimate/dist/package/dist/cli.js cargo test -p pimate-host agent_cli_contract` で、同じ模擬 RC に対する両 CLI の JSON・終了コードを検査できます。上記の `real_pi` 試験には、実物の Pi + faux を使った `prompt --wait` → `wait` → `history --last` も含みます。

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

## アプリ中核（`pimate-core`）

`Core::new` に OS の安全な保存先を実装した `SecretStore` を渡します。`pair(link, device_name)` はリンクの Host ID と `/pair` の応答を照合して保存し、公開の `Host` にはトークンを含めません。保存先は Host とトークンを同じレコードとして読み書きし、失効は同じ device ID のレコードにだけ適用します。`hosts()` で一覧、`connect(id, listener)` で Host ごとの接続を得ます。接続を閉じるにはその `Arc<Connection>` を解放します（端末から Host を消す場合はその後 `forget`）。通信は Tailscale 内の HTTP / WS、ネットワーク処理は中核の Tokio runtime 上です。

- `subscribe(Some(instance_id))` で表示する Pi を切り替え、`None` で解除します。`snapshot()` は現在の revision と全項目。`Listener::changed(revision, changes)` は `collection` と `id` で識別する更新（値と挿入位置）または削除です。UI スレッドへ順に転送し、取り直した snapshot 以前の revision は捨てます。`instance` は表示順で `section` が `waiting` / `normal` / `bot`。`entry`・`stream`・`question`・`run`・`tool`・`quota` は選択中の Pi の項目です。Markdown 本文は文字列のままです。
- `Target` は**画面を開いた時点の** instance / session ID。`check(operation)` で可否を確認し、`perform(operation).await` で実行します。送信前にも同じ条件を検査します。`Send` は最大4枚・RC の対応枚数と byte 制限を検査して常に steer 指定。`Abort`・`Answer`、モデルやセッションなどの `Rc`、起動・GitHub・端末管理・更新の Host 操作があります。複雑な引数は protocol v1 の params を表す JSON **文字列**（wire envelope ではありません）。中核が検査・ID 採番・ガード付与・応答の照合を行い、RC の成功時は method の result、Host の成功時は結果メッセージの JSON 文字列を返します。失敗は `Error` で、サーバーのエラー本文は転送しません。
- 切断後に自動で行うのは再認証と再購読です。**操作は再送しません**。結果不明の送信は `draft` として接続オブジェクトに残し、再同期時に同一セッションの新しい entry ID・本文・画像がすべて一致した時だけ照合します。別の内容になった画像は一致扱いしません。確認して破棄する場合は `DismissDraft`。入力中のテキストは UI の編集バッファと区別してください。
- プロバイダーの設定は `open_login(hold_id)` が専用 Pi を保持し、`hello` と `auth_list` で機能を確認します。認証はこの target へ `Rc`、進捗取得は `follow_login`、画面を閉じたら `EndAuthSession`。`session_stats` / `allowances(now_ms)` / `command_suggestions(text)` で UI 用の集計と候補を取得できます。

`cargo test -p pimate-core` は状態・ガードの単体試験、`cargo test -p pimate-host paired_core_recovers` は一時 Host＋Unix socket 模擬 RC の結合試験です。後者はプロンプト受理直後にループバックの TCP 中継を切り、復帰後も送信が一度だけであることと、履歴照合・質問への回答・失効を確認します。Android は NDK のパスを `ANDROID_NDK_HOME` に設定して `cargo ndk -t arm64-v8a check -p pimate-core`。UniFFI と iOS のビルド検証は [#14](https://github.com/phni3j9a/pimate-rs/issues/14)。

### ログと通知配送の契約

- WS の生フレームには端末トークン・プロバイダーの回答が入ります。自由記述の Close reason は受信・自動返信とも [tungstenite 0.29.0 の局所修正](vendor/tungstenite/PATCH.md) で DEBUG に出しません。別 workspace に組み込む場合もルートの `[patch.crates-io]` が必要です（適用漏れはビルドエラー）。通常のデータフレームについては、中核が依存する `log` と `tracing` の静的上限を **debug ビルドで DEBUG、release ビルドで INFO** にして TRACE をコンパイル時に取り除き、ビルド時と試験でも検査します。アプリや将来の FFI が TRACE のロガーを登録しても、生フレームのログは生成されません。Cargo の feature 統合により、同じ Rust バイナリの他の crate の TRACE も出ません。DEBUG / INFO は診断のために残します。UI 自身が秘密入力・ストレージ値を出力しないことは、別途 UI 実装の責任です。
- `Listener::changed` は接続ごとの専用スレッドで実行されます。配送待ちは最大8バッチです。UI が追いつかず `Change::ResyncRequired` が届いたら、部分適用せず `snapshot()` で全体を置換し、その snapshot の revision 以下の配送済み／配送待ち通知は無視してください。通常の Upsert は同じ ID があれば一度外し、指定位置に挿入します。
- `operationResult/latest` はリクエスト ID 付きの直近のセッション／圧縮操作結果です。受付後の snapshot に完了が含まれない場合、Host が直後に配送するイベントを最大5秒待ちます。この間は操作ロックを保持し、次の snapshot で期限を延ばしません。完了・失敗イベントが来なければ結果不明を一度だけ公開してロックを解放し、古い要求を照合候補に残しません。受付後の切断や、猶予中の別のセッション移動も結果不明です。`new` / `switch` / `reload` は送信後の移動理由と到着先を snapshot で確認できれば成功とします。`fork` は到着確認後もイベントを待ち、期限を過ぎた場合は成功だけを確定します（選んだ元の本文は `forkText`、RC から受信した `editorText` はイベントにのみ含まれます）。`tree` は同じセッションの snapshot だけでは移動を証明できません。**既知の制限:** 今の RC の完了イベントには要求 ID がないため、再同期をまたいだ操作や同じ種類の連続した操作は結果不明になることがあります。遅れて届いた成功・失敗を次の操作に割り当てず、未照合の件数だけを記録します。インスタンスの消失・再起動で件数を破棄し、未照合の最終操作から10分で失効させます。イベントが失われた後に同種の操作を10分以内ごとに続けると結果不明が続くことがありますが、10分空ければ解消します。その期限を過ぎて届いた古い完了は新しい操作と誤って照合する可能性があります。M7 で Extension の完了イベントに `requestId` を付けて解消します。現状は操作中・5秒の猶予中とも、他端末が同じ対象に行った同種操作の結果とは区別できません。停止と送信 ACK の順序が逆でも未消費の下書きを保持し、未確認のまま別の指示を手動送信できます。`subscribe` は不正な ID に `BadRequest` を返し、元の選択は変更しません。
