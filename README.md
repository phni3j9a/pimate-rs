# pimate-rs

[PiMate](https://github.com/phni3j9a/PiMate)（PC で動く [Pi Coding Agent](https://github.com/earendil-works/pi) をスマホなどから使うツール）の Rust 版です。最終的に TS 版を置き換えます。

**状態:** protocol v1 と Host 基盤（発見・ペアリング・認証・中継）を実装しました。スマホからの Pi の起動・PC での attach・ログイン専用 Pi にも対応しました。外部エージェント用 CLI・Webhook・GitHub 閲覧・Linux の常駐登録にも対応しています。Android 開発版では Rust Host にペアリングし、チャット、変更レビュー、サブエージェント、新しい Pi の起動、マシン管理、Issue / PR の閲覧ができます。他の OS と Extension の切替は今後の範囲です（[ロードマップ #4](https://github.com/phni3j9a/pimate-rs/issues/4)）。

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

### 会話の Markdown

[`pimate-markdown`](crates/pimate-markdown) の `parse(source, file_images)` は描画用の `Document` を返します。見出し・段落・表のセルは装飾付きテキストと画像の列、引用とリストは子 block を持ちます。`Block::plain_text()` は選択用の本文、`id..end` は元 Markdown の UTF-8 バイト範囲、`Table::markdown` は「表を Markdown でコピー」用です。`cell_width` / `table_widths` で TS 版と同じ列幅を計算できます。

`Document::images` は表示順の画像、`links` は http(s) リンクです。PC パスは `file_images` が true の場合だけ画像になります。解析自体は通信もファイルアクセスもしません。逐次表示では新しい全文を解析して `new.changes_since(&old)` を呼び、最上位 block の削除と追加・更新を ID 順に反映します（ID は同一メッセージ・同じ親の中で安定）。後から届く参照定義も反映するため全文を読み直す方式です。長い会話はチャンクをまとめて UI スレッドの外で解析してください。計測: `cargo run --release -p pimate-markdown --example stream_time`。

選択範囲の `id..end` は UTF-8 のバイト位置で、UTF-16 の添字ではありません。Swift/Kotlin では `pimate-ffi` の `utf16Range` を使います。コード全体のコピーには `Content::Code.text`、表示と部分選択にはタブを空白2個にした `plain_text()` を使います。欠けた表セルは格納しないため、UI では空として描きます。参照定義の画像はソースを共有し、分類は同じ定義につき一度だけ行います。

`pulldown-cmark` のイベントから描画用の木を作ります。記号・行数・入力バイト数では装飾を打ち切りません。実際の入れ子が **32 段**に達した部分、またはヘッダーを含む **行数×列数が 8,192 を超える表**だけを原文で表示します。これはネイティブのスタックと UI のセル数を保護するためです。パーサが疎な表に空セルを補う前に、幅の大きな表候補は少数列で実際の行範囲を確認します。これと別に、長い参照リンクや画像の代替文字列を繰り返し展開する追加メモリは文書あたり概算 **4 MiB** までとし、超過した参照だけを原文に戻します。ストレス計測は `cargo run --release -p pimate-markdown --example stress_time` です。

**TS 版（marked）との既知の差:** 表は `pulldown-cmark` の GFM 表拡張で認識します。marked に合わせて本文にパイプを補ったり、区切り行をマスクしたりしません。以下の `\n` は改行を表します。

- `項目\n|:--|\nりんご` は marked では1列表、Rust では段落です。1列でもヘッダーを `| 項目 |` と書けば両方で表になります。
- `- h\n  |:--|\n  a` は marked ではリスト内の1列表、Rust ではリスト項目の本文です。
- `a|b\n--:--|--\n1|2` は marked では段落ですが、`pulldown-cmark` 0.13.6 は途中にコロンのある区切りセルも受理し、2列表にします。双方で右揃えにするには `---:` と書いてください。

## Host を試す（Linux / macOS）

[`crates/pimate-host`](crates/pimate-host) は `pimate` 単一バイナリです。既存 Host と並べて試す場合、**両方の端末で同じ一時ディレクトリと別ポート**を指定してください。`list` / `serve` は runDir の古い登録を掃除します。

```sh
cargo build -p pimate-host
export PATH="$PWD/target/debug:$PATH"
lab=$(mktemp -d /tmp/pimate.XXXXXX); export PIMATE_RUN_DIR="$lab/run" PIMATE_CONFIG_DIR="$lab/config"
export PI_CODING_AGENT_DIR="$lab/agent" XDG_CONFIG_HOME="$lab/xdg"
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

ペアリング済み WS の `folders` / `dirs` で起動先を選び、`spawn` へ `cwd` と任意の `model`・`thinking`・`askTool`・`trust`・初回 `message` を渡します。受理した起動はスマホが切断しても初回メッセージ送信まで継続します。初回メッセージの失敗で Pi や prompt を自動再試行しません。終了は既存の RC `shutdown`。Rust Host は `PIMATE_RUN_DIR/t/`（0700）を端末専用の名前空間にします。同じ `pimate` バイナリの保持プロセスが Pi ごとに PTY と `<runDir>/t/<名前>` を持ち、Host を再起動・更新しても Pi は続きます。誰も attach していない間の出力は読み捨てます。`attach` は一度に1クライアントで、接続時に Pi を再描画し、端末のリサイズを反映します。名前指定の attach はそのパスだけを見ます。一覧には接続可能なソケットだけを表示し、接続中かどうかはモードから判定します。Pi が終了すると保持プロセス自身がソケットを回収します。

**既知の制限:** 保持プロセスの異常終了で残ったソケットは自動削除しません。TS 版から置き換える際、同じ runDir に互換 RC を登録した生存中の Pi は `list`・チャット・`shutdown` などを引き続き利用できます。旧 abduco セッションに Rust 版からは attach できません（`~/.abduco` は探索しません）。その端末には従来の CLI から接続するか、Pi を終了して Rust Host から起動し直してください。既存セッションを自動移行・終了することはありません。

`PIMATE_PI`（既定 `pi`）と TS Extension が必要です。`PIMATE_EXTENSION` はファイルパスまたは Pi のパッケージ指定（`npm:`、`git:` など）で、既定は配布パッケージの `dist/extension.js` です。Linux の user systemd が使えれば Pi ごとの scope、それ以外は独立した端末セッションを使います。後者はログアウトやサービスの cgroup 終了に対する保証はありません。

`auth_session` のログイン専用 Pi は接続・`holdId` ごとに保持し、`auth_session_end` または切断で解放します。通常の一覧・購読には出さず、`hello` と認証用 RC だけを許可します。起動待ちの保持を解放・切断すると、新たなログイン Pi は起動しません。すでに始まった起動は完了させます。最後の解放から60秒経過後、保存中なら延期し、対話中ならキャンセルして終了します。

上記の `real_pi -- --ignored` は WS 起動・初回 prompt・Host 再起動・PTY attach・専用 Pi の保持と終了まで検査します（1分強）。`serve` / `attach` / `list` には同じ短い絶対パスの `PIMATE_RUN_DIR` を指定してください。端末ソケットのパス、または RC の `<runDir>/<UUID>.sock` が OS のバイト長上限に達する場合、起動はエラーになります。

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

macOS ではログインしたユーザーで `pimate service install` を実行すると、`~/Library/LaunchAgents/dev.pimate.rs.host.plist` を作成して GUI ログインドメインに登録します。`pimate service status` で登録・稼働状態を確認し、`stop` / `start` で停止・再開、`uninstall` で登録と plist を削除します。別の設定の plist や同名の別ジョブは置換しません。WS のパッケージ更新・再起動は引き続き Linux の管理サービスだけが対象です。

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
- プロバイダーの設定は `open_login(hold_id)` が専用 Pi を保持し、`hello` と `auth_list` で機能を確認します。認証はこの target へ `Rc`、進捗取得は `follow_login`、画面を閉じたら `close_login(hold_id)`。`session_stats` / `allowances(now_ms)` / `command_suggestions(text)` で UI 用の集計と候補を取得できます。

`cargo test -p pimate-core` は状態・ガードの単体試験、`cargo test -p pimate-host paired_core_recovers` は一時 Host＋Unix socket 模擬 RC の結合試験です。後者はプロンプト受理直後にループバックの TCP 中継を切り、復帰後も送信が一度だけであることと、履歴照合・質問への回答・失効を確認します。Android は NDK のパスを `ANDROID_NDK_HOME` に設定して `cargo ndk -t arm64-v8a check -p pimate-core`。端末向けの生成と結合試験は下記の Swift / Kotlin の手順を参照してください。

### ログと通知配送の契約

- WS の生フレームには端末トークン・プロバイダーの回答が入ります。自由記述の Close reason は受信・自動返信とも [tungstenite 0.29.0 の局所修正](vendor/tungstenite/PATCH.md) で DEBUG に出しません。別 workspace に組み込む場合もルートの `[patch.crates-io]` が必要です（適用漏れはビルドエラー）。通常のデータフレームについては、中核が依存する `log` と `tracing` の静的上限を **debug ビルドで DEBUG、release ビルドで INFO** にして TRACE をコンパイル時に取り除き、ビルド時と試験でも検査します。アプリや将来の FFI が TRACE のロガーを登録しても、生フレームのログは生成されません。Cargo の feature 統合により、同じ Rust バイナリの他の crate の TRACE も出ません。DEBUG / INFO は診断のために残します。UI 自身が秘密入力・ストレージ値を出力しないことは、別途 UI 実装の責任です。
- `Listener::changed` は接続ごとの専用スレッドで実行されます。配送待ちは最大8バッチです。UI が追いつかず `Change::ResyncRequired` が届いたら、部分適用せず `snapshot()` で全体を置換し、その snapshot の revision 以下の配送済み／配送待ち通知は無視してください。通常の Upsert は同じ ID があれば一度外し、指定位置に挿入します。
- `operationResult/latest` はリクエスト ID 付きの直近のセッション／圧縮操作結果です。受付後の snapshot に完了が含まれない場合、Host が直後に配送するイベントを最大5秒待ちます。この間は操作ロックを保持し、次の snapshot で期限を延ばしません。完了・失敗イベントが来なければ結果不明を一度だけ公開してロックを解放し、古い要求を照合候補に残しません。送信後の切断（ACK 前を含む）や、猶予中の別のセッション移動も結果不明です。`new` / `switch` / `reload` は送信後の移動理由と到着先を snapshot で確認できれば成功とします。`fork` は到着確認後もイベントを待ち、期限を過ぎた場合は成功だけを確定します（選んだ元の本文は `forkText`、RC から受信した `editorText` はイベントにのみ含まれます）。`tree` は同じセッションの snapshot だけでは移動を証明できません。**既知の制限:** 今の RC の完了イベントには要求 ID がないため、再同期をまたいだ操作や同じ種類の連続した操作は結果不明になることがあります。RC が返す `sessionId` は完了時の値なので、セッションをまたいだ同種操作も結果不明になり得ます。送信後に実行の有無が不明なら、開始時の `instanceId`・`startedAt`・操作の種類ごとに未照合件数を記録し、遅着した成功・失敗を次の要求には割り当てません。確定した拒否は数えません。購読中は一覧と snapshot の到着順が前後し得るため、一覧の世代差・欠落では操作や件数を破棄しません。購読の `instance_gone` または新世代の snapshot で件数を破棄し、未照合の最終操作から10分で失効させます。イベントが失われた後に同種の操作を10分以内ごとに続けると結果不明が続くことがありますが、10分空ければ解消します。その期限を過ぎて届いた古い完了は新しい操作と誤って照合する可能性があります。M7 で Extension の完了イベントに `requestId` を付けて解消します。現状は操作中・5秒の猶予中とも、他端末が同じ対象に行った同種操作の結果とは区別できません。停止と送信 ACK の順序が逆でも未消費の下書きを保持し、未確認のまま別の指示を手動送信できます。`subscribe` は不正な ID に `BadRequest` を返し、元の選択は変更しません。

### Swift / Kotlin から使う（`pimate-ffi`）

UniFFI 0.32 のバインディングは **ネイティブライブラリと同じ checkout・Cargo.lock からビルド時に生成**します。大きな生成コードや OS/ABI 別バイナリをコミットせず、古いラッパーと新しい ABI を混ぜないためです。`pimate-bindgen` は開発機だけで使い、モバイルライブラリに `bindgen` feature は不要です。

- Swift: `let core = Core(store: keychain)` → `let host = try await core.pair(link: link, name: name)` → `let c = try core.connect(id: host.id, listener: listener)` → `try c.subscribe(instance: id)`。
- Kotlin（`uniffi.pimate_ffi`）: `val core = Core(store)` → coroutine 内で `val host = core.pair(link, name)` → `val c = core.connect(host.id, listener)` → `c.subscribe(id)`。接続を使い終わったら `c.close()`（Swift は ARC）。`perform`・`openLogin`・`followLoginOnce` も async / suspend です。
- `SecretStore` をアプリ側で実装し、Host メタデータと token を一緒に原子的に更新します（iOS は Keychain、Android は Keystore の鍵で暗号化したストレージなど）。`invalidate` は host ID **と device ID** が一致する場合だけ。保存側は `CallbackError.Failed` / `CallbackException.Failed()` を投げられます。想定外の例外も呼び出し元では詳細を含まない Storage エラーになります。`save`・`invalidate` は通信ワーカーから分離します。リンク・Credential・例外の詳細をログに出しません。
- `Listener.changed` は UI スレッドではありません。例外が出たら、失敗した revision で `ResyncRequired` を再配送します。後続の差分は不要で、1秒から上限16秒まで間隔を延ばし、成功または接続オブジェクトの解放で再試行を止めます。通信が失効・終了した後も、オブジェクトを保持していれば `snapshot()` を取れます。Swift で snapshot を取り直すリスナーは **接続への逆参照を weak に**してください（例: `weak var connection: Connection?`、アクセスを同一キューまたは lock で同期）。強参照では接続 → リスナー → 接続の循環で切断されません。接続ごとに revision 順を保って UI に配送し、`ResyncRequired` ではバッチを部分適用せず `snapshot()` で置換します（上記のログと通知配送の契約）。起動時に `initLogging()` を呼ぶと Android は logcat、Apple は os_log へ出力します。ログレベルの引数はなく、中核の静的上限と close reason の除去は変わりません。
- ログイン画面は開くたびに新しい hold ID を用意します。`openLogin` の戻り値は `Target`（値）で、解放に必要なのは **呼ぶ前から分かっている ID だけ**です。取得に失敗・キャンセルした場合も `closeLogin` を呼んでください。未取得・解放済みの ID なら何もしません。切断時には Host がそのソケットの全 hold を解放し、次の接続には持ち越しません。

```kotlin
val id = UUID.randomUUID().toString()
try {
    val target = c.openLogin(id)
    while (c.followLoginOnce(target)) { /* UI には Listener で反映 */ }
} finally { c.closeLogin(id) }  // suspend しない
```

```swift
let id = UUID().uuidString
// 画面の async 関数内。openLogin より先に登録する。
defer { c.closeLogin(holdId: id) }
let target = try await c.openLogin(holdId: id)
try await c.followLogin(target: target)
```

Swift の `withLogin` はこの defer を内包します。Task キャンセルは進行中の要求を中断しないため、解放まで応答またはタイムアウトを待つ場合があります。

- Markdown は `parse` の `Document` から `blocks()`・`images()`・`links()`・`changesSince(previous)` を取得します。大量に参照される画像 URL・href を block ごとに複製しないよう、`MarkdownImage.src`・`Style.href` は `SharedText` ハンドルです。文字列が必要なときだけ `value()` を呼びます。`utf16Range(source, start, end)`（Swift は引数ラベルあり）は元 Markdown の UTF-8 バイト範囲を UTF-16 の **開始・終了（終端を含まない）** に変換します。逆順・範囲外・文字途中は nil / null。Swift では `NSRange(location: start, length: end - start)` 相当で、`String.Index` の文字数ではありません。

**Android**（NDK 27、cargo-ndk 4.1.2）:

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install --locked cargo-ndk --version 4.1.2
ANDROID_NDK_HOME="$HOME/Android/Sdk/ndk/27.1.12297006" scripts/build-android.sh
```

`target/mobile/android/{jniLibs,kotlin}` を Android の source set に加え、JNA（Android 用 AAR、5.17.0）と kotlinx-coroutines-core を依存に追加します。

Kotlin の suspend API は Main 限定ではありません。生成時にすべての JNA コールバックへ `CallbackThreadInitializer(true, false)` を設定します。Rust ワーカーで継続がその場で再開して再び poll しても、内側のコールバックからは detach せず、ワーカー終了時に JNA が detach します。Java 所有スレッドは再 attach しません。`FutureContract` は模擬 Host 不要で、この再入、Main / Default / IO / 計装スレッドからの呼び出し、`SecretStore.save` の blocking ワーカーと接続を閉じた後の通知配信スレッドの終了を検査します。Swift は直接 C コールバックを使うのでこの設定は不要です。

**iOS**（Xcode のある macOS）:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
scripts/build-ios.sh
```

`target/mobile/ios/PiMate.xcframework`（iOS 16.0 以降の端末と Apple Silicon シミュレーターの静的ライブラリ・ヘッダー・module map）をリンクし、同じ場所の `pimate_ffi.swift` と `Login.swift` をアプリのターゲットでコンパイルします。

**接続の結合試験:** `scripts/test-mobile.sh swift`（macOS）または `scripts/test-mobile.sh kotlin`（Linux、JDK・`kotlinc` が必要）。後者は `KOTLIN_CP` に JNA 5.17.0・kotlin-stdlib・kotlinx-coroutines-core-jvm の JAR を `:` 区切りで指定します。どちらも一時ディレクトリ・ポート 0 の Rust Host と Unix socket の模擬 RC を起動し、ペアリングリンクは試験クライアントの標準入力にだけ渡します。実装例は [`crates/pimate-ffi/tests/native`](crates/pimate-ffi/tests/native)（試験専用のメモリ保存）です。

### Android 開発版と模擬 Host

`apps/android` は API 26 以上の Compose アプリです。上記の Rust ターゲット、cargo-ndk、Android SDK 36、JDK 17 以上（JRE のみの環境では `JAVA_HOME` を JDK に向ける）を用意し、`ANDROID_HOME`（NDK の版が異なる場合は `ANDROID_NDK_HOME` も）を設定して実行します。

```sh
(cd apps/android && ./gradlew assembleDebug testDebugUnitTest)
adb -s SERIAL install -r apps/android/app/build/outputs/apk/debug/app-debug.apk
scripts/android-demo.py
```

スクリプトは一時ディレクトリに模擬 RC と Rust Host を作り、loopback の空きポートだけで待機します。表示された `adb -s SERIAL reverse tcp:PORT tcp:PORT` を別端末で実行し、**PiMate RS** (`dev.pimate.app.rs.debug`) の最初の画面で「リンクを貼り付ける」に表示されたリンクを入れるか、QR を読み取ります。リンクは秘密・1回限りなので別端末で試す時はスクリプトを起動し直します。終了は Ctrl+C、そのポートだけ `adb -s SERIAL reverse --remove tcp:PORT`。実際の Pi・常駐サービス・abduco は使いません。

模擬の Pi はモック（[docs/design](docs/design)）と見比べられる内容にしています。

| 一覧の表示 | 見るところ |
| --- | --- |
| `android`（`/demo/android`） | 開くと Markdown（見出し・表4種・コード・リスト・タスクリスト・引用・リンク）が逐次届く。送信すると、考えた内容 → ツール4回 → 回答 → 確認の質問、の順に進む |
| `--shout フラグを追加` | `Main` の状態。畳まれた前のターン、ツールの行、実行中の `bash`。止めるまで作業中のまま |
| `ログイン画面の改修` | `Chat-Waiting` の状態。要対応に入り、選択肢つきの質問が出る |
| `認証まわりのレビュー`・`README の整理` | `Drawer` の補足（サブエージェント 2 実行中・スマホで起動） |

Rust 版のデザインの正本は [docs/design](docs/design) と [docs/SPEC.md](docs/SPEC.md)。画面は `PairScreen.kt`（最初の画面）、`MainActivity.kt`（ドロワーと、設定・新しい Pi の入口）、`ChatScreen.kt`（見出し・会話の行・入力欄）、`QuestionCard.kt`、`Markdown.kt`（表・コード・選択とコピー）。色・文字・余白は `Theme.kt`、アイコンと共通の部品は `Parts.kt`、履歴を行に組み立てて完了したターンを畳む処理は `Transcript.kt`（JVM の単体試験 `TranscriptTest`）。通信の境界は `SessionModel.kt` と `EncryptedStore.kt`。生成された Kotlin・.so は編集しません。等幅の JetBrains Mono（SIL OFL 1.1、`assets/licenses`）を同梱しています。

端末試験は `apps/android/app/src/androidTest`。`StoreContract` はランダムな試験用 Host だけで Keystore の置き換えを検査します。`UiContract` はドロワーの幅・会話内の表のリンク・Markdown の解析を UI スレッドの外で行うことを検査します（模擬 Host 不要）。`DemoJourney` は初回ペアリング画面からの試験用です（アプリのデータが空で、模擬 Host を起動し直した直後に実行します）。debug APK と `assembleDebugAndroidTest` の APK を試験端末に入れ、上記の reverse 後、模擬リンクを標準入力で `adb -s SERIAL shell "run-as dev.pimate.app.rs.debug sh -c 'mkdir -p cache; cat > cache/demo-link'"` へ渡します（履歴やログに残さない）。`adb -s SERIAL shell am instrument -w dev.pimate.app.rs.debug.test/androidx.test.runner.AndroidJUnitRunner` で実行できます。リンクがなければ会話試験はスキップします。

開発用会話で作業中に割り込み、消費される前に停止すると「割り込み待ち」→「届いていない可能性があります」になります。同じセッションで入力欄が空なら本文を下書きへ戻せますが、自動再送はしません。編集本文は Host・Instance・セッションごとに ViewModel のメモリだけで保持します（画面回転・一時切断では保持、プロセス終了で消去）。質問カードは `QuestionCard.kt` で5種類を描画し、回答の送信可否は中核に委ねます。Android 12 以降のクラウドバックアップ・端末間転送は `hosts.xml` を除外します。

未確認の復元は `Connection.takeUnconfirmed(target, id)` で行います。現在の対象・未確認状態を中核の同じロック内で照合し、本文と画像を返して draft を削除します。遅着した履歴と照合済み・対象違いなら失敗し、UI の編集内容は変更しません。確定した拒否は操作エラーとして通知し、復元可能な draft には残しません。Android の入力イベントは表示時の編集キーに限定し、質問の書きかけも `ChatEdits.kt` のメモリ上で Host・対象・requestId ごとに保持します。同じ対象の質問一覧から消えたときに破棄します。

### Android チャットの状態と操作（#19 前半）

`SessionModel.rowsFor(host.id)` は再登録が必要な Host を除く全マシンの独立した接続・revision・一覧です。チャットの `rows` と `target` は `select(host, instanceId)` で選んだものだけ。マシンをまたいで同じ Instance／セッション ID があっても、編集キーには必ず Host が入ります。要対応は全マシンの `section == "waiting"` を先頭にまとめます。

| 画面で使う情報 | 読み書きの契約 |
| --- | --- |
| 一覧の補足 | `instance` の任意の `questionTitle`・`failureText`。TS Host の一覧には件数・状態しかない場合があります。Rust Host は一覧のために履歴を取得せず、`hello` の `pendingUI` から質問の題だけを取得。中核は購読中に観測した親の失敗だけを一覧に反映し、詳細がなければ定型文を使います。子の累積失敗数は現在の要対応ではありません。 |
| 時間・途中出力 | `view/runningSince`・`tool.startedAt` は epoch ms。`thinkingMs/<assistant entryId>` は最初の thinking delta から最初の本文（なければ回答確定）まで。RC イベントの任意の `timestamp` を優先し、時刻がない v1 は TS 版と同じく受信時刻、途中参加の作業は観測開始から。途中参加の思考時間や見ていないツール出力は推定しません。`tool.partial.content` は最新の途中出力。再購読時は同世代・同セッションで履歴上も同じ未完了の呼び出しのときだけ復元（Host ごとに最近の16 Instance。途中出力はテキストと照合情報のみを全 Host 合計512 KiBまで保持し、上限を超える呼び出しは破棄。ディスク保存なし）。 |
| 添付 | 画面がシステムの `PickMultipleVisualMedia(4)` を起動し、起動時の `EditorKey` と結果 URI リストを `attach(key, uris)` に渡します。キャンセルは空リスト。能力と枚数を確認し、IO スレッドで縮小・向き補正・JPEG 化し、1枚512 KiB・全体600 KiB以内に収まった場合だけ `edits.body.images` に反映。読み取り・サイズエラーは画面で案内し、リンクや画像をログに出しません。 |
| 結果が必要な RC 操作 | キーは画面を開いたときに捕捉し、coroutine で `request(key, RcMethod, JSONObject)`。`ENTRY_IMAGE`・`FILE_IMAGE` は返された result JSON の画像を利用。`MODELS`／`SET_SCOPED_MODELS`／`SET_DEFAULT_MODEL` の結果は `model` の `favorite` と `view/defaultModel`、モデル・Thinking の変更は `SET_MODEL`／`SET_THINKING`。`SESSIONS`／`SESSION_TREE` は `session`／`node`、変更は `SESSION_OP`・`SET_LABEL`・`COMPACT`・`SHUTDOWN`。対象が変われば結果を別画面に渡さず、自動再送しません。 |
| 候補・使用状況 | `COMMANDS` 後に `suggestions(text)`。`stats()` と `allowances(nowMs)` は中核の集計。`view/context`、`quota`、圧縮・セッション操作の `operationResult/latest` も参照できます。 |
| 再接続・PC 操作 | `host/connection`、`view/phase`、`draft`、`notice/sessionChanged`（`reason`, `from`, `to`。どの端末から操作したかは不明）、`view/pcPrompt`。`attachCommand()` が null ならコピーを出しません。未確認は入力欄・履歴と区別し、`restore` で明示的にだけ戻します。 |

制限: 未購読中の失敗は RC v1 の snapshot から識別できません。最新の失敗を Extension の snapshot に載せる対応は M7 で行います。

2マシンの計装試験は `PIMATE_DEMO_NAME='マシン A' scripts/android-demo.py` と B を別々に起動し、各ポートだけ reverse。初回状態の RS debug アプリの `cache/two-links` にリンクを1行ずつ渡し、`DemoJourney#twoMachines` を実行します。`global-attention.png` と `two-machines.png` をアプリの cache から取得し、終了後に各プロセスと reverse を片付けます。

### Android の残りの画面で使う状態（#20 前半）

`SessionModel` は Main スレッドから使います。画面を開いた時点の Host ID（会話なら `EditorKey`）を捕捉し、閉じる・対象を変えるときはその画面の coroutine をキャンセルしてください。以下は画面から使う入力と操作の契約です。

| 画面 | 状態と操作 |
| --- | --- |
| 変更レビュー | `review(entries, cwd, turn)` に `entry` 行の JSON を履歴順で渡す（重い履歴は UI スレッド外で解析）。`turn` は user entry ID、null は読み込み済みセッション全体。`latestChangedTurn`、ファイル別の `pieces`（呼出順）、元の patch・hunk・行番号・追加削除数、差分なしの理由を返す。最初の piece の `created` が作成の根拠。write の本文だけでは追加行数を数えない。`askAbout(key, review)` は画像・既存本文を残して参照を書き足すだけ。自動送信しない。 |
| サブエージェント | `run` の `open`（停止中／再開可能を含む）・`active`・`children`・`stopAction`。`rows.subagentCounts()` は観測した実行の完了後も次のユーザー発言まで要約を残す。再購読時には同世代・同セッション・同じ user entry の場合だけ snapshot に残る run を照合。子の画面を開くごとに `workRecord(key,run,child)` を作り、`load()` で不透明な cursor を最大8ページ追う。`more` なら続きを手動で、load が true を返したら取得中に更新があったため再取得。`control(key,run,child,action,text)` は現時点の `actions` を中核が検査。子の指示は `childId` と `text`、run 全体の停止・interrupt・abort_parent は childId を付けず、影響範囲を確認してから。resume の結果は `newRunId`。受付後だけ `instructions.forChild` に送信者ラベル用の本文・操作をメモリ保持（Host・会話・run・child 別、全体128件かつキー等を含む65,536 UTF-16単位まで。超過時は古い順に破棄し、単独で超える指示は記録しない）。 |
| 新しい Pi | `SpawnForm(hostId,settings.askTool)` はマシン切替で作り直す。`machineRequest(id,Operation.Folders)` の home・folders、`Dirs(path)` の候補、`folder(path, knownTrusted)`（信頼を確認したパスは空白も含めそのまま送る）、`hostModels(id)` の models/scoped/default。チャットは購読し直さず最大3つの Pi へ一覧を照会。Pi がない場合も PC の既定で起動可。`spawn(form)` は trust・askTool・任意の model/thinking/message を一度だけ送る。初回本文の結果不明・20秒以内に一覧に現れない起動を区別し、Ready のときだけ `select` 可能。 |
| 設定・情報 | `hosts`、`settings.chooseAskTool`・`chooseScale`・`chooseTheme(SYSTEM/LIGHT/DARK)`。既存の文字倍率設定を引き継ぎ OS 倍率に乗算。テーマ変更は全体に即時反映。版はインストール済み APK の versionName。`checkAppUpdate(version)` は Rust 版 `REPO` の最新の公開 `app-vX.Y.Z` を照会する（配布前・通信失敗は Unknown、TS 版 APK は案内しない）。`REPORT_ISSUE` は同じリポジトリ。 |
| マシン詳細 | `rowsFor(id)` の `host/connection` と `host/info`（能力）。`machineRequest` に `UpdateCheck/Update/Restart/GithubStatus/Devices/Revoke(deviceId)`。成功は Host の結果 JSON、失敗・切断は例外。端末の取り消しと `forget(id)`（このスマホの暗号化された登録・接続・編集を削除）は別操作。確認後だけ実行。 |
| プロバイダー | 画面の coroutine 内の `withLogin(id) { login -> … }` は専用 Pi の hold を finally で解放。`rowsFor(id)` の provider/auth 行（キーは `login.target.instance`）、`login.request(AUTH_BEGIN/RESPOND/CANCEL/LOGOUT,params)` と `followOnce()`。working/input/saving 中だけ接続・ログイン先につき1つの wait（重複は送信せず Blocked）、変更後は `AUTH_LIST` で再取得。リンクは中核が検証した authorization だけを明示タップで開く。ブラウザでの認証中に再接続した場合も、接続の世代が変われば専用 Pi と一覧を取り直します。差分を取り逃した再同期では古い hold を再利用しません。秘密入力は保存・ログ出力・再送しません。資格情報の保存結果が不明な場合と、保存後に Pi の再読み込みが必要な場合を区別します。 |
| GitHub | `github(key,query)` の `kind=list` または `kind=issue/pr,number`。Host が対象 Pi のリポジトリを決める。非対応・未ログインは空一覧ではなくエラーとして扱う。 |
| Pair／セットアップ／最初の Pi | QR・貼付などの候補は `previewPair(link)` で名前・アドレスを検査してから `pair(link)`。案内ページ・カメラ・外部リンクは画面層。追加登録経路は `showPairing(true/false)`、SavedState に保存するのはフラグだけ。初回成功のみ `pairedHostId` が入る（画面で受け取ったら `consumePairedHost()`）。`rowsFor(id).firstStep()` は loading/offline/empty/list。接続済みでも最初の一覧到着前は empty としない。 |

### Android の画面と試し方

- Pair の手順 1・2 からセットアップの案内を開けます。リンク・QR の名前とアドレスを確認してペアリングし、初回は起動中の Pi を選ぶか新しい Pi を起動します。2台目以降は一覧に戻ります。
- チャットの **… → 変更レビュー** でターンまたは読み込み済みセッションの差分を開きます。「この変更について依頼」は下書きに参照を加えるだけです。同じメニューから現在の Pi のリポジトリの Issue / PR を読みます。「Pi に依頼」は既存の下書き・画像を残して参照を追加するだけです。本文・コメント・CI の省略は明示し、詳細から GitHub を開けます。子の状態は入力欄の上の1行から開きます。指示は対象の子が対応する場合だけ。停止は確認時の範囲に固定し、対象や操作が変われば確認を閉じます。再開先の子は新しい run の到着後に選び直します。
- ドロワーの **新しい Pi** ではマシン・フォルダ・信頼・モデル・初回本文を指定できます。**設定** にはマシンごとの管理、テーマ・文字サイズ・質問ツールの既定、APK の版と更新があります。端末の取り消しは PC 側の操作、マシンの削除はこのスマホの登録・下書きの削除です。通知は作りません。
- 実装は `Pages.kt`（セットアップ・初回）、`ConversationPages.kt`（レビュー・子の作業記録・GitHub）、`MachinePages.kt`（起動・設定・マシン・専用 Pi の認証）。いずれも既存の画面と同じテーマで、操作に使う ID は画面の対象に固定します。

**専用の画面試験**: 既定の `scripts/android-demo.py` は今までどおり Rust Host と模擬 Pi だけです。保守・ログイン・GitHub の成功状態も安全に試す場合だけ `PIMATE_DEMO_NAME='デモ PC' PIMATE_DEMO_SCREENS=list scripts/android-demo.py` を使用します（Node.js と Node から `require('ws')` できる環境。なければ一時ディレクトリで `npm install --prefix "$lab" ws@8`、`NODE_PATH="$lab/node_modules"`）。`android-screen-fixture.cjs` は loopback だけで待ち、ペアリングとチャットは専用 Rust Host、保守・GitHub・認証は固定のテスト応答です。systemctl・更新・実認証を呼びません。空一覧は `PIMATE_DEMO_SCREENS=empty`。再接続の試験は `PIMATE_DEMO_SCREENS=reconnect`・`cache/reconnect-link`・`RemainingJourney#loginHoldRecoversAfterDisconnect` で、最初の認証一覧のあとに一度だけ接続を閉じます。登録のない**専用 APK** と1回限りのリンクを使い、上記 `demo-link` の手順で `cache/ui-link`（空は `cache/empty-link`）に渡します。模擬保守を使う場合だけ `cache/mock-maintenance` に空ファイルを用意し、`RemainingJourney#screens`（空は `#firstEmpty`）を実行します。実 Pi の `#realFaux` は `cache/real-link` と、`検証用 PC` という名前の一時 Host・`work` フォルダ内の本物の Pi + faux が必要です。各試験はリンクがなければスキップします。

状態の端末試験: `SettingsContract` はテーマ／文字／質問ツールの保存と新しい ViewModel への追加ペアリングの復元・削除を検査。`MachineJourney` は未登録の RS debug アプリ＋上の模擬 Host 専用で、reverse 後に `cache/machine-link` に同じ手順でリンクを渡します。チャット未選択のままモデル・端末を取得し、レビューの参照を既存下書きに加えるところまで確認します（送信・実サービス操作なし）。
