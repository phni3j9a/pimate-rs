# pimate-rs

[PiMate](https://github.com/phni3j9a/PiMate)（PC で動く [Pi Coding Agent](https://github.com/earendil-works/pi) をスマホなどから使うツール）の Rust 版です。最終的に TS 版を置き換えます。

**状態:** TS 版との機能差は Android・iOS・macOS・Linux でおおむね解消しました（[#74](https://github.com/phni3j9a/pimate-rs/issues/74)）。デスクトップは[画面設計の段階1〜5](docs/design/desktop/PLAN.md)を終え、会話を中心に作り直しました（[#73](https://github.com/phni3j9a/pimate-rs/issues/73)）。

公開済みは [Host・npm 0.2.0](https://github.com/phni3j9a/pimate-rs/releases/tag/host-v0.2.0)、[Android 0.2.1](https://github.com/phni3j9a/pimate-rs/releases/tag/app-v0.2.1)、[Linux デスクトップ 0.2.0](https://github.com/phni3j9a/pimate-rs/releases/tag/desktop-v0.2.0) です。[次の版のリリースノート（下書き）](docs/release-notes/)は未公開です。iOS の TestFlight と macOS の署名・公証は [#70](https://github.com/phni3j9a/pimate-rs/issues/70) で扱います。

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

### RC Extension

[`packages/pimate`](packages/pimate) は Pi 内で動く `@phni3j9a/pimate` です。wire 型は `src/protocol.ts` から Rust の生成物を参照します。リポジトリのルートから:

```sh
(cd packages/pimate && npm ci && npm run typecheck && npm run build)
(
  lab=$(mktemp -d /tmp/pimate.XXXXXX); trap 'rm -rf "$lab"' EXIT
  export TMPDIR="$lab" PIMATE_RUN_DIR="$lab/run" PIMATE_CONFIG_DIR="$lab/config"
  export PI_CODING_AGENT_DIR="$lab/agent" XDG_CONFIG_HOME="$lab/xdg" PIMATE_SERVICE_PORT=47801
  cd packages/pimate && npm test
)
```

出力は `packages/pimate/dist/extension.js`。開発時の `PIMATE_EXTENSION` はこの絶対パス、`/pimate` の管理コマンドには `PIMATE_HOST_BINARY` を **この checkout の Rust バイナリの絶対パス**に設定します。未指定時は npm パッケージの `bin/<platform>-<arch>/pimate` のみを実行し、別のインストールを PATH から探しません。配布版の Host は逆にこの配置から `dist/extension.js` を解決します。

Extension と移した試験は PiMate `d940de8` の MIT ライセンス（`packages/pimate/LICENSE`）。`diff`・`qrcode` 等の第三者コードは dist にバンドルせず npm 依存としてライセンスを保持します。abduco は同梱しません。Grok Bot 用の4つの Skill とテンプレートの構成は `grok-bot/` に同梱し、Skill・Webhook の派生元のライセンスは `notices/T3-Tots.txt` に収録します。

### npm 配布と TS 版からの切り替え（M8 で公開を承認した後）

パッケージ名は旧公開版と同じ `@phni3j9a/pimate`。Node.js 22 系は 22.19 以降、23 系は 23.11 以降、または 24 以降が必要です（CLI は `process.execve` を使用）。npm の `bin.pimate` は `dist/cli.js` から OS・CPU に合った `bin/{linux,darwin}-{x64,arm64}/pimate` に exec します。Pi がインストールした npm の bin が PATH にない場合、従来どおり `/pimate setup` の確認に同意すると同梱 Host へ `~/.local/bin/pimate`（XDG 上書き可）をリンクします。既存の別ファイルは上書きしません。

1. **更新前**に旧 abduco 端末で進行中の仕事を確認。Rust Host は旧端末を attach・移行しません。完了を待つか旧 CLI で切り離した仕事を保存して Pi を終了し、必要なら更新後に起動し直してください。互換 RC が生きている Pi は同じ runDir ならチャットできますが、端末自体への接続には旧 CLI が必要です。
2. 旧 Host を **手動で起動した場合は先に元の端末から停止**します。旧 TS 版の `/pimate status` で使用中の CLI リンクのパスと Host の版を控え、TS 版をロードしたまま `/pimate uninstall` を実行します。Host 停止・unit・リンク削除の成功通知を確認してください。Linux では `${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/pimate-host.service` が消え、どの OS でも控えたリンク（通常 `~/.local/bin/pimate`）が消えたことを確認します（各パス `p` に対し `test ! -e "$p" && test ! -L "$p"` で壊れたリンクも検出）。エラーや管理外ファイルがあれば更新せず、所有者を確かめてから旧版で再試行します。**Pi パッケージは remove せず、設定・ペアリング・runDir・`~/.abduco` も削除しません。**
3. `pi update npm:@phni3j9a/pimate`（新規は `pi install npm:@phni3j9a/pimate`）。Pi を再起動し `/pimate setup` で Host と CLI リンクを設定。Linux は systemd の常駐登録に同意、macOS の常駐は `pimate service install`。`/pimate status` で Host / Extension の版・稼働状態を確認します。新しいペアリングは `/pimate pair`。**パッケージの置換自体に再インストールは不要ですが、TS の unit は Node の旧 CLI を指すので初回だけサービスの移行が必要です。**

**TS 版へ戻す場合:** Rust が起動した Pi の仕事を保存して終了し、手動の Rust Host があれば元の端末で停止します。**まだ Rust 版がインストールされている間に** `/pimate uninstall` を実行し、成功通知、Host の停止、上記の unit（macOS は `~/Library/LaunchAgents/dev.pimate.rs.host.plist`）と CLI リンクの消失を確認します。失敗したまま旧版を入れないでください。`pi remove npm:@phni3j9a/pimate` → `pi install npm:@phni3j9a/pimate@0.1.20`（例。実際は控えた TS 版を指定）→ `pi list` で版固定を確認し、Pi を再起動して TS 版の `/pimate setup` でサービス・リンクを再作成します。`/pimate status` と既存スマホからの接続を確認してください。ペアリング設定は削除不要です。

### ビルド・リリース手順（公開しない検証が既定）

[Host npm package](.github/workflows/package.yml) は同じコミット・Cargo.lock から4対象を release build します。Linux は同じ CPU の runner で musl を静的リンクし、動的ローダーがないことを検査するため配布先の glibc に依存しません。macOS は Apple Silicon 上で両 arch をビルドし、arm64 の ad-hoc 署名を検証します（Developer ID 署名・公証ではありません）。

1. `crates/pimate-host/Cargo.toml` と `packages/pimate/package.json` および両 lockfile の版をそろえます。CI は不一致を拒否。Host の `--version` と `/health` はバイナリ自身の版、更新比較は起動時の同梱 manifest の版です。`bin/SHA256SUMS.json` は最終バイナリ4個の SHA256 で、独立した署名ではありません。
2. ローカルでは CI から同じソースツリーの binary artifact（各 `pimate` とビルド時の `build.json`）を `packages/pimate/bin/<platform>-<arch>/` に置き、次を実行。組み立て時に対象・版・Git ツリー・SHA256 を再照合し、違う成果物の混入を拒否します。`crates/`・`vendor/`・Cargo manifest/lock・`.cargo/`・Rust toolchain ファイルに未コミットの変更（未追跡・ステージ済みを含む）があれば、先にコミットするかクリーンな worktree を使ってください。開発用 manifest は private のまま、配布用 manifest だけを生成します。組み立て環境には Rust・`cargo-about` 0.9.3 が必要です。`notices/` に Host の依存 crate・Rust 標準ライブラリ・[musl](https://git.musl-libc.org/cgit/musl/tree/COPYRIGHT)・Webhook の派生元 T3 Tots のライセンスを収録します。

   ```sh
   cd packages/pimate && npm ci && npm run package
   (cd dist/package && npm pack --pack-destination ..)
   python3 scripts/pack-smoke.py dist/phni3j9a-pimate-$(node -p 'require("./package.json").version').tgz
   ```

3. PR・main・手動実行（`publish=false`）は pack → 隔離 Pi でスモーク → Actions artifact `npm-package` まで。公開権限・トークンを使用しません。
4. M8 の公開前チェックリスト（この PR では公開しません）:
   - 所有者と `npm latest` の置換・版・移行日時・ロールバック先・Release 添付を確認し、対象コミットが `main` に統合された後で `host-v<version>` タグを用意します。タグ push は公開トリガーではありません。
   - GitHub Environment **`npm-release`** の deployment branch policy が「branch `main` のみ、tag なし」であることを確認。repository secret や、この repo から使える organization secret に `NPM_TOKEN` は置きません。**古いタグ側の workflow は `npm-release` を指定しておらず、指定しても tag は許可されないため、Environment の秘密を取得できません。**
   - 推奨: 所有者が npm の `@phni3j9a/pimate` の設定で [Trusted Publisher](https://docs.npmjs.com/trusted-publishers/) に GitHub Actions（owner `phni3j9a`、repository `pimate-rs`、workflow `package.yml`、environment `npm-release`）を登録。`auth=oidc` は公開ジョブのみの `id-token: write` を使い、長期トークンは不要です。**OIDC が使えない場合のみ**必要最小限・短命の `NPM_TOKEN` を `npm-release` の Environment secret に設定し、明示的に `auth=token` を選びます。OIDC 失敗時の token 自動再試行はありません。
   - Actions の **Use workflow from: `main`** を選び、入力 `release_tag=host-v<version>`、まず `publish=false` で検証。その後、公開承認が取れたら同じ入力と `publish=true` で実行。main 側のガードがタグ名・`origin/main` の履歴・タグ内 npm/Cargo manifest を照合し、検証済み SHA を checkout してビルドします。公開直前にもタグ・ビルド SHA・tgz の版を照合し、不一致や認証失敗なら Release も作りません。
   - npm の公開後に Release のみ失敗した場合は npm を再公開せず、その実行の `npm-package` artifact を使って Release だけを復旧します。

## Host を試す（Linux / macOS）

[`crates/pimate-host`](crates/pimate-host) は `pimate` 単一バイナリです。既存 Host と並べて試す場合、**両方の端末で同じ一時ディレクトリと別ポート**を指定してください。`list` / `serve` は runDir の古い登録を掃除します。

```sh
cargo build -p pimate-host
export PATH="$PWD/target/debug:$PATH"
lab=$(mktemp -d /tmp/pimate.XXXXXX); export PIMATE_RUN_DIR="$lab/run" PIMATE_CONFIG_DIR="$lab/config"
export PI_CODING_AGENT_DIR="$lab/agent" XDG_CONFIG_HOME="$lab/xdg"
export PIMATE_EXTENSION="$PWD/packages/pimate/dist/extension.js" PIMATE_HOST_BINARY="$PWD/target/debug/pimate"
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

通常の `cargo test` は一時ディレクトリ・ループバックの自動割当ポートで模擬 RC を使います。実物の Pi 1.6.0 と上記でビルドした Extension があれば、下記で PTY 上の Pi を起動し、発見・購読・prompt・faux モデルの返答まで検査できます。ネットワークの LLM・既存 Host・既存 Pi の設定は使いません。

```sh
(
  lab=$(mktemp -d /tmp/pi.XXXXXX); trap 'rm -rf "$lab"' EXIT
  export TMPDIR="$lab" PIMATE_RUN_DIR="$lab/run" PIMATE_CONFIG_DIR="$lab/config"
  export PI_CODING_AGENT_DIR="$lab/agent" XDG_CONFIG_HOME="$lab/xdg" PIMATE_SERVICE_PORT=47801
  mkdir "$lab/bin"; for cmd in systemctl systemd-run loginctl; do ln -s /usr/bin/false "$lab/bin/$cmd"; done
  PATH="$lab/bin:$PATH" PIMATE_TEST_PI="$HOME/.pi/agent/bin/pi" \
    cargo test -p pimate-host --test host real_pi -- --ignored
)
```

### スマホからの起動とログイン

ペアリング済み WS の `folders` / `dirs` で起動先を選び、`spawn` へ `cwd` と任意の `model`・`thinking`・`askTool`・`trust`・初回 `message` を渡します。受理した起動はスマホが切断しても初回メッセージ送信まで継続します。初回メッセージの失敗で Pi や prompt を自動再試行しません。起動の失敗の `code` は、何も起動していない `not_found`・`not_directory`・`bad_request`・`busy`（ほかの起動の最中・直後）・`spawn_failed` と、保持のプロセスを起動したあとで準備の完了を確認できなかった `outcome_unknown`（Pi が動いているかもしれないので、一覧を確かめてから再実行する）を区別します。終了は既存の RC `shutdown`。Rust Host は `PIMATE_RUN_DIR/t/`（0700）を端末専用の名前空間にします。同じ `pimate` バイナリの保持プロセスが Pi ごとに PTY と `<runDir>/t/<名前>` を持ち、Host を再起動・更新しても Pi は続きます。誰も attach していない間の出力は読み捨てます。`attach` は一度に1クライアントで、接続時に Pi を再描画し、端末のリサイズを反映します。名前指定の attach はそのパスだけを見ます。一覧には接続可能なソケットだけを表示し、接続中かどうかは同一 UID の保持プロセスへの短い状態照会で判定します。照会自体は接続状態・サイズ・画面を変更しません。Pi が終了すると保持プロセス自身がソケットを回収します。

`PIMATE_ABDUCO_SESSION` と RC `hello` の `abducoSession` は通信互換のため残した歴史的な名前で、abduco のインストールは不要です。

**既知の制限:** 保持プロセスの異常終了で残ったソケットは自動削除しません。TS 版から置き換える際、同じ runDir に互換 RC を登録した生存中の Pi は `list`・チャット・`shutdown` などを引き続き利用できます。旧 M1 Rust Host が `runDir/t` に作った abduco ソケットも一覧・attach の対象外です。旧 abduco セッションに Rust 版からは attach できません（`~/.abduco` は探索しません）。その端末には従来の CLI から接続するか、Pi を終了して Rust Host から起動し直してください。既存セッションを自動移行・終了することはありません。

`PIMATE_PI`（既定 `pi`）と TS Extension が必要です。`PIMATE_EXTENSION` はファイルパスまたは Pi のパッケージ指定（`npm:`、`git:` など）で、既定は配布パッケージの `dist/extension.js` です。Linux の user systemd が使えれば Pi ごとの scope、それ以外は独立した端末セッションを使います。後者はログアウトやサービスの cgroup 終了に対する保証はありません。

`auth_session` のログイン専用 Pi は接続・`holdId` ごとに保持し、`auth_session_end` または切断で解放します。通常の一覧・購読には出さず、`hello` と認証用 RC だけを許可します。起動待ちの保持を解放・切断すると、新たなログイン Pi は起動しません。すでに始まった起動は完了させます。最後の解放から60秒経過後、保存中なら延期し、対話中ならキャンセルして終了します。

上記の `real_pi -- --ignored` は WS 起動・初回 prompt・Host 再起動・PTY attach・専用 Pi の保持と終了まで検査します（1分強）。`serve` / `attach` / `list` には同じ短い絶対パスの `PIMATE_RUN_DIR` を指定してください。端末ソケットのパス、または RC の `<runDir>/<UUID>.sock` が OS のバイト長上限に達する場合、起動はエラーになります。

### 外部エージェントと Webhook

Grok Bot から使うには、公開テンプレート **[PiMate](https://x.ai/bot/EL8LzPmNquZTjEawTxAI1)** を追加します。Bot は PC 上の Skill を自動で読みません。テンプレートは旧版向けなので、今回の Skill を含むパッケージの導入・更新後は、Bot に `~/.pi/agent/npm/node_modules/@phni3j9a/pimate/grok-bot/skills/` の各 `SKILL.md` を読み、同じ名前・説明のプライベート Skill として全文を保存し直すよう依頼してください。新しい PC の確認は `pimate-getting-started`、質問をチャットで受け取る任意の Routine の設定は [テンプレート構成](packages/pimate/grok-bot/TEMPLATE.md)を参照してください。

`spawn --cwd DIR [-- pi args]` は `origin: "grok-bot"` の新しい Pi を作り、`instanceId`・`attachCommand` などを JSON で返します。`prompt ID TEXT` は `--steer` / `--follow-up` を選択でき、`TEXT` が `-` なら標準入力を読みます。`abort ID` はターンを中断、`shutdown ID` は Pi を終了します。`watch ID` は RC の生イベントを表示します。

```sh
pimate prompt "$id" 'この変更を確認して' --wait --timeout 60
pimate wait "$id" --redact --timeout 60  # Bot に渡す結果は伏せ字に。timeout でも再送しない
pimate history "$id" --last 4        # 直近の会話を古い順で返す
```

`outcome` は `done` / `failed` / `aborted` / `waiting_input` / `timeout` / `idle` / `closed`。`waiting_input` の `pending` に対して `ui ID REQUEST <値|--confirm|--cancel|--answers JSON>` で回答します。通常の値と `--answers` は原文一致専用です。Webhook カードは `ui ID REQUEST --label LABEL`、質問票は `--answer-labels JSON` を使います。PC 内で全候補を表示用に変換し、一意に一致するときだけ送信します。カードに回答した後は Webhook の続きを待ち、`wait` を呼びません。`wait`・`prompt --wait` は既定では原文を返し、`--redact` を付けた場合だけ Webhook と同じ伏せ字を適用します。Bot が監視外の Pi を待つときは必ず `--redact` を付けます。平文の質問への返信前は `history ID --last 1` の最新行の `entryId` と完了通知の `lastReplyEntryId` で同じ発言か確認できます。`--wait` は既定300秒。結果に含まれる `failed`・`timeout` と、CLI 自体の失敗（終了コード1）は異なります。

Webhook は任意です。`webhook setup [--machine 表示名]` で URL とヘッダー（`Authorization: ...`）を端末の標準入力から渡します。`webhook test` は実際に送信します。`webhook watch ID` で明示した Pi の質問・解決・完了・失敗・子の待機などを送ります。`status` には資格情報を出しません。解除は `webhook unwatch ID`、設定ごとの削除は `webhook off` です。秘密の伏せ字は完全ではなく、送信先には質問や回答の一部が届きます。初回送信の100枠が埋まった場合、新規通知はログに記録して破棄します。配信済みまたは配信中の質問に対する解決通知だけは、質問の配信成功後に別タスクで枠を待ち、Pi の監視を止めません。再送キューはメモリ内のみ。アプリへの push 通知は実装しません。

### 管理と常駐登録

認証済み WS の `github` は対象の Pi の作業ディレクトリに限り Issue / PR を読みます。`github_status` は github.com のログイン名だけを取得します。`update_check` は npmjs.org の最新版を確認し、`update` は公開 npm パッケージに同梱され、かつ `pimate-host.service` で稼働する場合だけ固定引数で更新します。`host_restart` はこの常駐サービスだけが対象です。比較元は起動時の npm 版（パッケージ外は crate の版・更新不可）です。更新と再起動は端末をまたいで排他し、競合時は更新に `busy`、再起動に `failed` を返します。`gh` の stderr には認証情報が含まれ得るため、TS 版とは異なり、失敗は分類コードと固定文言のみを返します。

Linux の user systemd の登録・確認・削除は `pimate service install|status|uninstall`。`start|stop|health|has-systemd` と `command-status|command-install|command-remove` も `service` のサブコマンドです。登録・削除は実際にサービス状態を変えます。**稼働中の TS Host と同じポート・サービス名で試さないでください。** `XDG_CONFIG_HOME` を変えても systemd の user manager は分離されません。Rust の管理印・設定ディレクトリと異なる unit は上書きしません。linger の設定は行いません。

macOS ではログインしたユーザーで `pimate service install` を実行すると、`~/Library/LaunchAgents/dev.pimate.rs.host.plist` を作成して GUI ログインドメインに登録します。`pimate service status` で登録・稼働状態を確認し、`stop` / `start` で停止・再開、`uninstall` で登録と plist を削除します。ログイン中の実 launchd で異常終了後の Host 復帰と、同じ Pi の継続・再 attach を[確認しました](docs/devin-macos.md#host-の-launchagent-を確認する)。起動エラーは非公開の設定ディレクトリの `host.log` に記録します。別の設定の plist や同名の別ジョブは置換しません。WS のパッケージ更新・再起動は引き続き Linux の管理サービスだけが対象です。

Extension のために `setup-info`、`pair --json [--address host:port]`（秘密のリンクと失効時刻）、`pair-pending`（コードは標準入力のみ）、`devices`、`revoke ID --json` と各 `service` 操作が JSON を返します。Extension はこれらの JSON を使い、Host の資格情報・管理処理を再実装しません。テストは管理コマンドを偽物、Webhook を loopback に切り替えます。`PIMATE_SERVICE_PORT` はサービス管理・health 確認用のポート（通常 47800）です。

TS の配布用 CLI がビルド済みなら、`PIMATE_TEST_TS_CLI=/path/to/PiMate/packages/pimate/dist/package/dist/cli.js cargo test -p pimate-host agent_cli_contract` で、同じ模擬 RC に対する両 CLI の JSON・終了コードを検査できます。上記の `real_pi` 試験には、実物の Pi + faux を使った `prompt --wait` → `wait` → `history --last` も含みます。

## 決まったこと

- アプリは iOS / Android / Linux / macOS の4つの OS に対応する
- WebView を使う方式（Dioxus の WebView 描画、Tauri など）は、アプリの質が下がるので使わない
- Linux も macOS と同じ GPUI を使う。Wayland の二重確定を修正し、IBus/Mozc と実 GPU の逐次描画を検証した（[#2](https://github.com/phni3j9a/pimate-rs/issues/2)）
- Host は端末の保持（今の abduco の役割）も内蔵する。段階を踏んで進める
  1. protocol を Rust の crate にする（[#1](https://github.com/phni3j9a/pimate-rs/issues/1)）
  2. Host を Rust に移植する（端末の保持は、いったん abduco を使う）
  3. 端末の保持を自前の実装に置き換える
- Host のバイナリは OS と CPU ごとに用意し、Pi のパッケージ（npm）に同梱する。利用者が Rust を入れる必要はない
- 通知は当面作らない
- iOS / macOS のビルドと確認は Devin Cloud の macOS VM で行う（手順は [docs/devin-macos.md](docs/devin-macos.md)、道具は `scripts/devin-mac/`。TS 版から移した）

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
- `operationResult/latest` はリクエスト ID 付きの直近のセッション／圧縮操作結果です。受付後の snapshot に完了が含まれない場合、Host が直後に配送するイベントを最大5秒待ちます。この間は操作ロックを保持し、次の snapshot で期限を延ばしません。完了・失敗イベントが来なければ結果不明を一度だけ公開してロックを解放し、古い要求を照合候補に残しません。送信後の切断（ACK 前を含む）や、猶予中の別のセッション移動も結果不明です。このリポジトリの Extension は `completionRequestIds: true` を通知し、完了・失敗イベントに元の RC 要求の `requestId` を付けます。他端末・遅着した別 ID の完了は無視し、セッション移動だけでは成功としません。`editorText`・`forkText` は成否とは別に宛先を検査します。tree は開始セッション、fork は Pi の `withSession` が返した分岐先に一致しないと本文を渡しません。旧 Extension の fork で完了が先着した場合、成否は直ちに通知し、本文だけは Host・Instance ごとに1件保持します。同世代の次の snapshot で観測済みの分岐先を確認したときだけ、同じ結果 ID に本文を補います。行き先の不一致、同じ Pi での別操作開始、10分の失効では破棄します。旧 Extension の成否だけは従来通り、操作種別・同世代の未照合件数・移動先で判定します。曖昧な結果は不明ですが、他端末からの同種操作や10分の失効後に遅着した完了は区別できません。停止と送信 ACK の順序が逆でも未消費の下書きを保持し、未確認のまま別の指示を手動送信できます。`subscribe` は不正な ID に `BadRequest` を返し、元の選択は変更しません。

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
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
scripts/build-ios.sh
```

`target/mobile/ios/PiMate.xcframework`（iOS 16.0 以降の端末と Apple Silicon・Intel シミュレーターの静的ライブラリ・ヘッダー・module map）をリンクし、同じ場所の `pimate_ffi.swift` と `Login.swift` をアプリのターゲットでコンパイルします。

**接続の結合試験:** `scripts/test-mobile.sh swift`（macOS）または `scripts/test-mobile.sh kotlin`（Linux、JDK・`kotlinc` が必要）。後者は `KOTLIN_CP` に JNA 5.17.0・kotlin-stdlib・kotlinx-coroutines-core-jvm の JAR を `:` 区切りで指定します。どちらも一時ディレクトリ・ポート 0 の Rust Host と Unix socket の模擬 RC を起動し、ペアリングリンクは試験クライアントの標準入力にだけ渡します。実装例は [`crates/pimate-ffi/tests/native`](crates/pimate-ffi/tests/native)（試験専用のメモリ保存）です。

### Android release APK（TS 版の上書き更新）

リポジトリルートで `scripts/build-android-release.sh` を実行します。下記の開発環境に加え、Build Tools 36.0.0 と GitHub Release を読める `gh` が必要です。`~/.config/pimate-release/` に **TS 版で使用中の** `pimate-release.jks` と `signing.properties` がそろっていることを確認してください。鍵は作成・変更せず、Gradle に渡すのは外部 properties のパスだけです。鍵がなければ中止します。[TS 版の署名鍵のバックアップ手順](https://github.com/phni3j9a/PiMate/blob/main/apps/mobile/README.md#署名鍵を必ずバックアップする)を参照してください。CI は鍵を使わず debug のみをビルドします。

- `apps/android/version.txt` が版の正本です。安定版の3要素（各 0〜999）を指定し、`versionCode = major × 1,000,000 + minor × 1,000 + patch` が既公開版より大きくなるようにします。初回は 0.2.0（2000）、TS 版 0.1.23（1023）の続きです。
- `:app:assembleRelease` は Rust を release ビルドしてから `dev.pimate.app` の APK を作ります。ARM 実機（arm64-v8a / armeabi-v7a）と x86_64 エミュレーターを1本に含めます。従来の Rust アプリと同じ API 26 以上で、TS 版の API 24–25 と 32 bit x86 専用端末は更新対象外です。UniFFI / JNA の呼び出しを保つため R8 とリソース縮小は無効のままです。
- スクリプトは一時取得した [TS 版 0.1.23](https://github.com/phni3j9a/PiMate/releases/tag/app-v0.1.23) とビルド結果を `apksigner verify --print-certs` で検証し、証明書 SHA-256 が一致した APK だけを `apps/android/dist/pimate-<版>.apk` にコピーします。表示された証明書と APK の SHA-256 を公開時の照合に使います。
- ランチャーアイコンは [docs/design/icon-kit](docs/design/icon-kit) の `android/native-light`（adaptive・単色のテーマアイコン・legacy）で、TS 版と同じライトの構成です。表示名は release が「PiMate」、debug（`dev.pimate.app.rs.debug`）が「PiMate RS」。iOS も表示名は「PiMate」、AppIcon は icon-kit の Light／Dark／Tinted です。
- APK を上書きインストールしてください（アンインストール不要）。TS 版のホストや設定は移行しません。旧 SecureStore のファイルがあれば未接続の初回画面に再ペアリングを案内します。旧ファイルの中身・鍵・DB は読み取らず削除もしません。Rust 版は別の Keystore alias・`hosts` と `display` の設定を使います。

**M8 の公開担当者へ:** 検証した commit の `pimate-rs` Releases に `app-v<版>` の正式リリースを作成し、検証済み APK・両 SHA-256・対応端末・再ペアリング手順を掲載します。Rust 版の「更新の確認」はインストール済みの `versionName` とこのリポジトリの `app-v…` を比較します。一方 TS 版が開けるのは旧 `phni3j9a/PiMate` Releases だけです。そちらにも同じ **正式** `app-v<版>` で同一 APK（再ビルドせず SHA-256 を照合）、移行案内、新リポジトリへのリンクを掲載してください。PC パッケージ用の `v…` タグを使わず、公開済み APK を差し替えません。

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

Rust 版のデザインの正本は [docs/design](docs/design) と [docs/SPEC.md](docs/SPEC.md)。画面は `PairScreen.kt`（最初の画面）、`MainActivity.kt`（ドロワーと、設定・新しい Pi の入口）、`ChatScreen.kt`（見出し・会話の行・入力欄）、`QuestionCard.kt`、`Markdown.kt`（表・コード・選択とコピー）。色・文字・余白は `Theme.kt`、アイコンと共通の部品は `Parts.kt`、履歴を行に組み立てて完了したターンを畳む処理は `Transcript.kt`（JVM の単体試験 `TranscriptTest`）。通信の境界は `SessionModel.kt` と `EncryptedStore.kt`。生成された Kotlin・.so は編集しません。等幅の JetBrains Mono（SIL OFL 1.1）と、ドロワーの設定の歯車に使う Lucide の `settings`（ISC）のライセンスを `assets/licenses` に同梱しています。

端末試験は `apps/android/app/src/androidTest`。`StoreContract` はランダムな試験用 Host だけで Keystore の置き換えを検査します。`UiContract` はドロワーの幅・会話内の表のリンク・Markdown の解析を UI スレッドの外で行うことを検査します（模擬 Host 不要）。`DemoJourney` は初回ペアリング画面からの試験用です（アプリのデータが空で、模擬 Host を起動し直した直後に実行します）。debug APK と `assembleDebugAndroidTest` の APK を試験端末に入れ、上記の reverse 後、模擬リンクを標準入力で `adb -s SERIAL shell "run-as dev.pimate.app.rs.debug sh -c 'mkdir -p cache; cat > cache/demo-link'"` へ渡します（履歴やログに残さない）。`adb -s SERIAL shell am instrument -w dev.pimate.app.rs.debug.test/androidx.test.runner.AndroidJUnitRunner` で実行できます。リンクがなければ会話試験はスキップします。

開発用会話で作業中に割り込み、消費される前に停止すると「割り込み待ち」→「届いていない可能性があります」になります。同じセッションで入力欄が空なら本文を下書きへ戻せますが、自動再送はしません。編集本文は Host・Instance・セッションごとに ViewModel のメモリだけで保持します（画面回転・一時切断では保持、プロセス終了で消去）。質問カードは `QuestionCard.kt` で5種類を描画し、回答の送信可否は中核に委ねます。Android 12 以降のクラウドバックアップ・端末間転送は `display.xml` だけを対象とし、新旧の認証情報を除外します。

未確認の復元は `Connection.takeUnconfirmed(target, id)` で行います。現在の対象・未確認状態を中核の同じロック内で照合し、本文と画像を返して draft を削除します。遅着した履歴と照合済み・対象違いなら失敗し、UI の編集内容は変更しません。確定した拒否は操作エラーとして通知し、復元可能な draft には残しません。Android の入力イベントは表示時の編集キーに限定し、質問の書きかけも `ChatEdits.kt` のメモリ上で Host・対象・requestId ごとに保持します。同じ対象の質問一覧から消えたときに破棄します。

### Android チャットの状態と操作（#19 前半）

本文で右にドラッグし、一覧の幅の半分を越えると、ゆっくり離しても一覧が開きます。横長の表の中では表を横送りできます。回答を末尾で読んでいる間は、逐次表示で行が増えても下端を固定します。履歴の閲覧や手動の展開では「最新の位置へ」を押すか末尾まで戻るまで、読んでいる位置を保ちます。

`SessionModel.rowsFor(host.id)` は再登録が必要な Host を除く全マシンの独立した接続・revision・一覧です。チャットの `rows` と `target` は `select(host, instanceId)` で選んだものだけ。マシンをまたいで同じ Instance／セッション ID があっても、編集キーには必ず Host が入ります。要対応は全マシンの `section == "waiting"` を先頭にまとめます。

| 画面で使う情報 | 読み書きの契約 |
| --- | --- |
| 一覧の補足 | `instance` の任意の `questionTitle`・`failureText`。TS Host の一覧には件数・状態しかない場合があります。Rust Host は一覧のために履歴を取得せず、`hello` の `pendingUI` と `failureSummary` を使用。失敗要約は親のエラーを認証・利用制限・タイムアウト・その他の固定文に分類し、本文・キー・プロンプトを入れません。Host と中核でも240文字までに制限。購読中の観測結果を優先し、未購読なら同世代・同セッションの有効な観測値、なければ一覧の要約を使います。子の累積失敗数は現在の要対応ではありません。 |
| 時間・途中出力 | `view/runningSince`・`tool.startedAt` は epoch ms。`thinkingMs/<assistant entryId>` は最初の thinking delta から最初の本文（なければ回答確定）まで。RC イベントの任意の `timestamp` を優先し、時刻がない v1 は TS 版と同じく受信時刻、途中参加の作業は観測開始から。途中参加の思考時間や見ていないツール出力は推定しません。`tool.partial.content` は最新の途中出力。再購読時は同世代・同セッションで履歴上も同じ未完了の呼び出しのときだけ復元（Host ごとに最近の16 Instance。途中出力はテキストと照合情報のみを全 Host 合計512 KiBまで保持し、上限を超える呼び出しは破棄。ディスク保存なし）。 |
| 添付 | 画面がシステムの `PickMultipleVisualMedia(4)` を起動し、起動時の `EditorKey` と結果 URI リストを `attach(key, uris)` に渡します。キャンセルは空リスト。能力と枚数を確認し、IO スレッドで縮小・向き補正・JPEG 化し、1枚512 KiB・全体600 KiB以内に収まった場合だけ `edits.body.images` に反映。読み取り・サイズエラーは画面で案内し、リンクや画像をログに出しません。 |
| 結果が必要な RC 操作 | キーは画面を開いたときに捕捉し、coroutine で `request(key, RcMethod, JSONObject)`。`ENTRY_IMAGE`・`FILE_IMAGE` は返された result JSON の画像を利用。`MODELS`／`SET_SCOPED_MODELS`／`SET_DEFAULT_MODEL` の結果は `model` の `favorite` と `view/defaultModel`、モデル・Thinking の変更は `SET_MODEL`／`SET_THINKING`。`SESSIONS`／`SESSION_TREE` は `session`／`node`、変更は `SESSION_OP`・`SET_LABEL`・`COMPACT`・`SHUTDOWN`。対象が変われば結果を別画面に渡さず、自動再送しません。 |
| 候補・使用状況 | `COMMANDS` 後に `suggestions(text)`。`stats()` と `allowances(nowMs)` は中核の集計。`view/context`、`quota`、圧縮・セッション操作の `operationResult/latest` も参照できます。 |
| 再接続・PC 操作 | `host/connection`、`view/phase`、`draft`、`notice/sessionChanged`（`reason`, `from`, `to`。どの端末から操作したかは不明）、`view/pcPrompt`。`attachCommand()` が null ならコピーを出しません。未確認は入力欄・履歴と区別し、`restore` で明示的にだけ戻します。 |

旧 Extension には要約がないため、未購読で観測値もない Pi の失敗は表示しません。

2マシンの計装試験は `PIMATE_DEMO_NAME='マシン A' scripts/android-demo.py` と B を別々に起動し、各ポートだけ reverse。初回状態の RS debug アプリの `cache/two-links` にリンクを1行ずつ渡し、`DemoJourney#twoMachines` を実行します。`global-attention.png` と `two-machines.png` をアプリの cache から取得し、終了後に各プロセスと reverse を片付けます。

### Android の残りの画面で使う状態（#20 前半）

`SessionModel` は Main スレッドから使います。画面を開いた時点の Host ID（会話なら `EditorKey`）を捕捉し、閉じる・対象を変えるときはその画面の coroutine をキャンセルしてください。以下は画面から使う入力と操作の契約です。

| 画面 | 状態と操作 |
| --- | --- |
| 変更レビュー | `review(entries, cwd, turn)` に `entry` 行の JSON を履歴順で渡す（重い履歴は UI スレッド外で解析）。`turn` は user entry ID、null は読み込み済みセッション全体。`latestChangedTurn`、ファイル別の `pieces`（呼出順）、元の patch・hunk・行番号・追加削除数、差分なしの理由を返す。最初の piece の `created` が作成の根拠。write の本文だけでは追加行数を数えない。`askAbout(key, review)` は画像・既存本文を残して参照を書き足すだけ。自動送信しない。 |
| サブエージェント | `run` の `open`（停止中／再開可能を含む）・`active`・`children`・`stopAction`。`rows.subagentCounts()` は観測した実行の完了後も次のユーザー発言まで要約を残す。再購読時には同世代・同セッション・同じ user entry の場合だけ snapshot に残る run を照合。子の画面を開くごとに `workRecord(key,run,child)` を作り、`load()` で不透明な cursor を最大8ページ追う。`more` なら続きを手動で、load が true を返したら取得中に更新があったため再取得。`control(key,run,child,action,text)` は現時点の `actions` を中核が検査。子の指示は `childId` と `text`、run 全体の停止・interrupt・abort_parent は childId を付けず、影響範囲を確認してから。resume の結果は `newRunId`。受付後だけ `instructions.forChild` に送信者ラベル用の本文・操作をメモリ保持（Host・会話・run・child 別、全体128件かつキー等を含む65,536 UTF-16単位まで。超過時は古い順に破棄し、単独で超える指示は記録しない）。 |
| 新しい Pi | `SpawnForm(hostId,settings.askTool)` はマシン切替で作り直す。`machineRequest(id,Operation.Folders)` の home・folders、`Dirs(path)` の候補、`folder(path, knownTrusted)`（信頼を確認したパスは空白も含めそのまま送る）、`hostModels(id)` の models/scoped/default。チャットは購読し直さず最大3つの Pi へ一覧を照会。Pi がない場合も PC の既定で起動可。`spawn(form)` は trust・askTool・任意の model/thinking/message を一度だけ送る。初回本文の結果不明・20秒以内に一覧に現れない起動を区別し、Ready のときだけ `select` 可能。 |
| 設定・情報 | `hosts`、`settings.chooseAskTool`・`chooseScale`・`chooseTheme(SYSTEM/LIGHT/DARK)`。既存の文字倍率設定を引き継ぎ OS 倍率に乗算。テーマ変更は全体に即時反映。版はインストール済み APK の versionName。`checkAppUpdate(version)` は Rust 版 `REPO` の最新の公開 `app-vX.Y.Z` を照会する（配布前・通信失敗は Unknown、TS 版 APK は案内しない）。`REPORT_ISSUE` は同じリポジトリ。 行の形・余白・アイコンは TS 版 0.1.23 の設定・マシン詳細に合わせる（1行 52dp・補足つき 60dp）。更新は設定を開いた時に確認する。通知の項目は当面作らない。 |
| マシン詳細 | `rowsFor(id)` の `host/connection` と `host/info`（能力）。`machineRequest` に `UpdateCheck/Update/Restart/GithubStatus/Devices/Revoke(deviceId)`。成功は Host の結果 JSON、失敗・切断は例外。マシン削除では接続中かつ `pairedDevices` 対応なら `core.revokeAndForget(connection, id, device)` を使う。未接続・非対応・拒否・結果不明の場合は、別途確認してからのみ `core.forgetDevice(id, device)` で手元だけ消す。 |
| プロバイダー | 画面の coroutine 内の `withLogin(id) { login -> … }` は専用 Pi の hold を finally で解放。`rowsFor(id)` の provider/auth 行（キーは `login.target.instance`）、`login.request(AUTH_BEGIN/RESPOND/CANCEL/LOGOUT,params)` と `followOnce()`。working/input/saving 中だけ接続・ログイン先につき1つの wait（重複は送信せず Blocked）、変更後は `AUTH_LIST` で再取得。リンクは中核が検証した authorization だけを明示タップで開く。ブラウザでの認証中に再接続した場合も、接続の世代が変われば専用 Pi と一覧を取り直します。差分を取り逃した再同期では古い hold を再利用しません。秘密入力は保存・ログ出力・再送しません。資格情報の保存結果が不明な場合と、保存後に Pi の再読み込みが必要な場合を区別します。 |
| GitHub | `github(key,query)` の `kind=list` または `kind=issue/pr,number`。Host が対象 Pi のリポジトリを決める。非対応・未ログインは空一覧ではなくエラーとして扱う。 |
| Pair／セットアップ／最初の Pi | QR・貼付などの候補は `previewPair(link)` で名前・アドレスを検査してから `pair(link)`。案内ページ・カメラ・外部リンクは画面層。追加登録経路は `showPairing(true/false)`、SavedState に保存するのはフラグだけ。初回成功のみ `pairedHostId` が入る（画面で受け取ったら `consumePairedHost()`）。`rowsFor(id).firstStep()` は loading/offline/empty/list。接続済みでも最初の一覧到着前は empty としない。 |

### Android の画面と試し方

- Pair の手順 1・2 からセットアップの案内を開けます。リンク・QR の名前とアドレスを確認してペアリングし、初回は起動中の Pi を選ぶか新しい Pi を起動します。2台目以降は一覧に戻ります。
- チャットの **… → 変更レビュー** でターンまたは読み込み済みセッションの差分を開きます。「この変更について依頼」は下書きに参照を加えるだけです。同じメニューから現在の Pi のリポジトリの Issue / PR を読みます。「Pi に依頼」は既存の下書き・画像を残して参照を追加するだけです。本文・コメント・CI の省略は明示し、詳細から GitHub を開けます。子の状態は入力欄の上の1行から開きます。指示は対象の子が対応する場合だけ。停止は確認時の範囲に固定し、対象や操作が変われば確認を閉じます。再開先の子は新しい run の到着後に選び直します。
- ドロワーの **新しい Pi** ではマシン・フォルダ・信頼・モデル・初回本文を指定できます。**設定** にはマシンごとの管理、テーマ・文字サイズ・質問ツールの既定、APK の版と更新があります。マシンを削除すると、対応する PC の自端末登録を先に取り消し、スマホの接続情報と下書きを消します。取消結果が不明なら PC の `/pimate devices` を確認してください。マシン詳細ではオフライン時の再接続、認証失効時の再ペアリング、`gh auth login` のコピーもできます。通知は作りません。
- 実装は `Pages.kt`（セットアップ・初回）、`ConversationPages.kt`（レビュー・子の作業記録・GitHub）、`MachinePages.kt`（起動・設定・マシン・専用 Pi の認証）。いずれも既存の画面と同じテーマで、操作に使う ID は画面の対象に固定します。

**専用の画面試験**: 既定の `scripts/android-demo.py` は今までどおり Rust Host と模擬 Pi だけです。保守・ログイン・GitHub の成功状態も安全に試す場合だけ `PIMATE_DEMO_NAME='デモ PC' PIMATE_DEMO_SCREENS=list scripts/android-demo.py` を使用します（Node.js と Node から `require('ws')` できる環境。なければ一時ディレクトリで `npm install --prefix "$lab" ws@8`、`NODE_PATH="$lab/node_modules"`）。`android-screen-fixture.cjs` は loopback だけで待ち、ペアリングとチャットは専用 Rust Host、保守・GitHub・認証は固定のテスト応答です。systemctl・更新・実認証を呼びません。空一覧は `PIMATE_DEMO_SCREENS=empty`。再接続の試験は `PIMATE_DEMO_SCREENS=reconnect`・`cache/reconnect-link`・`RemainingJourney#loginHoldRecoversAfterDisconnect` で、最初の認証一覧のあとに一度だけ接続を閉じます。登録のない**専用 APK** と1回限りのリンクを使い、上記 `demo-link` の手順で `cache/ui-link`（空は `cache/empty-link`）に渡します。模擬保守を使う場合だけ `cache/mock-maintenance` に空ファイルを用意し、`RemainingJourney#screens`（空は `#firstEmpty`）を実行します。実 Pi の `#realFaux` は `cache/real-link` と、`検証用 PC` という名前の一時 Host・`work` フォルダ内の本物の Pi + faux が必要です。各試験はリンクがなければスキップします。

状態の端末試験: `SettingsContract` はテーマ／文字／質問ツールの保存と新しい ViewModel への追加ペアリングの復元・削除を検査。`MachineJourney` は未登録の RS debug アプリ＋上の模擬 Host 専用で、reverse 後に `cache/machine-link` に同じ手順でリンクを渡します。チャット未選択のままモデル・端末を取得し、レビューの参照を既存下書きに加えるところまで確認します（送信・実サービス操作なし）。

`PIMATE_DEMO_CHAT_FAILURE=1 scripts/android-demo.py` は `api` の親チャットにサブエージェントの失敗履歴を追加します（iOS の `AgentReviewJourney` では `TEST_RUNNER_PIMATE_DEMO_CHAT_FAILURE=1` も指定）。

### iOS 開発版

Xcode 16.3 以降、iOS 17+。TS 版とは別の `dev.pimate.app.rs.debug` を使います。macOS で `brew install xcodegen` と前述の Rust 3 ターゲットを用意し、`apps/ios/prepare.sh` → `open apps/ios/PiMate.xcodeproj`。Rust や binding の変更後も prepare を実行します。XcodeGen は差分が読める `apps/ios/project.yml` だけを管理し、同じ checkout でビルドした XCFramework（arm64 実機／arm64 + x86_64 simulator）と Swift を組み込みます。生成プロジェクト、バイナリ、テスト結果はコミットしません。

`apps/ios/test-disposable.sh` が CI と同じビルド・XCTest・画面撮影を実行します。認証試験には Node.js と npm が必要です（模擬 Host 用の `ws` は既存 lockfile の版を試験用の一時ディレクトリに取得）。`simctl create` で新規の iPhone 15 を作成し、成功・失敗によらず今回の UDID だけを削除します。既存の UDID を `test.sh` に直接渡す方法は、開発版アプリを上書きするため非推奨です。試験起動は毎回空の試験専用 Keychain サービスを使います。一時ディレクトリの loopback 模擬 Host で登録→一覧→日本語送信→履歴到着を通し、終了時に模擬 Host とシミュレーターを止めます。UI 試験は専用 Host のリンクをシミュレーターのペーストボードに渡して、Pair → 一覧 → 会話 → 質問への回答を操作します。CI の `ios-comparison` はモック｜ライト｜ダーク｜OS の文字が大きい場合の WebP です。専用モックのない画像・表・コマンド候補は3つの表示条件を並べます。Devin は [既存 VM の使い方](docs/devin-macos.md)に従い、`use` → 必要な分だけ `keepalive` → `sync` → 同期先で同じ `apps/ios/test-disposable.sh` を実行。個人の認証情報は転送せず、終了時は待機を終了し `devin-mac down`。

初回は QR（タップしたときだけカメラを許可）またはリンクで接続先の名前・アドレスを確認してから登録します。写真は対応する Pi に最大4枚まで添付でき、サムネイルの×で個別に取り消せます。会話中の画像は表示範囲に入ると読み込みます。表のセルの画像も、表を開いてからタップすると回答内の画像を順番に閲覧できます。画像はタップで全画面にし、スワイプか前後のボタンで切り替えます。HTTP 画像は自動取得せずリンクで開きます。`/` の候補はタップで入力するだけで送信しません。先頭に空白がある ` /new` などは操作ではなく本文として送ります。PC 専用の画面はターミナルでの操作を案内し、安全な attach コマンドがある場合にだけコピーできます。iPhone は Pi 一覧から会話に入り、戻る操作で切り替えます。iPad は標準のサイドバー。設定は一覧のスライダーです。質問には会話内で回答。本文は長押しで部分コピー、表はタップで拡大して選択、コードは全体コピーもできます。Return は改行・変換確定だけで、送信（作業中は割り込み）は矢印。

`SessionModel` は App 所有で scene の再生成後も維持。マシン別の `hosts` / `rows[id].groups` と `select(host:instance:)`、表示時に捕捉した `editor` と `body/text/send/attach/abort/restore`、`QuestionKey(editor:requestId:)` と表示時の質問 JSON を渡す `answer` を使ってください。`attach` に渡すのは選択時のキーとローカルの一時ファイル URL（完了まで保持）。1枚ずつ縮小します。`draft` は履歴と区別し `restore` は明示操作だけ。確認の途中で別の対象に変わったら閉じ、送信失敗時にも自動再送しません。`SessionModel`・`Rows`・`Photo`・`KeychainStore`、FFI と生成物を画面から変更しないでください。実カメラでの読み取りは実機確認が必要です。

会話の右上の「会話の操作」から、モデル・Thinking（PC と共通のお気に入り／新しい Pi の既定）、使用状況（コンテキスト・この PC の Claude / Codex 利用枠）、セッションを開きます。セッションでは新規・再開・分岐・圧縮・ツリーからの移動とラベルの保存／解除、確認付きの Pi 終了と attach コマンドのコピーができます。`/model`・`/resume`・`/fork`・`/tree` を送信しても該当するシートを開きます。対象が切り替わればシートを閉じ、操作の受付と結果を区別します。Pi の作業中も再開先や分岐元を閲覧できますが、会話の切り替えはできません。保存先の分からない会話は終了時に履歴を失う可能性を案内します。不明な利用枠や操作結果は推測しません。

### iOS チャット状態（#41）

- **添付**: ピッカーを開く時の `EditorKey` を `attach(_:files:)` に渡します。`attaching` の間は追加・送信不可。ローカル一時ファイルを1枚ずつ縮小し、最大4枚、1枚512 KiB・計600 KiB以内。`removeAttachment(_:shown:at:)` に表示時の画像列を渡して1枚だけ削除。`restore(_:id:)` は画像ごと空の入力欄へ戻すだけで送信しません。
- **履歴・Markdown 画像**: `HistoryImages(row).source(at:thumbnail:canFetch:)` は元の画像位置を維持し、プレビューのない位置を `entry_image` で取得可。`ChatImageSource(markdown:answer:)` は**画面に入った画像だけ**で生成し、回答の安定した ID を渡します（逐次本文や SharedText のインスタンスを ID にしない）。`image(_:source:thumbnail:)` は圧縮済み `Data` を返し、同じ参照を共有します。`file_image` は回答間で共有しません。Web 画像は cookie・資格情報を付けずメモリでのみ取得します。HTTPS は ATS を維持し、画像1件の上限は20秒・8 MiB・HTTPS リダイレクト5回です。LRU は使用時に更新。表示終了で Swift Task を cancel、画面では縮小して復号してください。UniFFI に送信済みの要求を撤回する API はなく、キャンセル後の返答を表示・保存しません。
- **Android との既知の差**: iOS の Markdown は HTTP 画像を自動取得せず、タップできるリンクとして表示します。
- **シート**: `openSheet(_:page:)` の戻り値で `run(_:in:)`。App 所有の `sheet` がページ・読み込み・返答・エラーを保持するので、scene 再生成時に開き直さずそのまま描画。`closeSheet(_:)`。`SheetAction` がモデル選択・Thinking・お気に入りの一覧・既定・セッション一覧/新規/再開/分岐/ツリー移動/ラベル/圧縮/終了を型付きで表します。`rows[host].groups` の `model`・`session`・`node`、`view/defaultModel`・`view/context`、`stats(_:)`・`allowances(_:nowMs:)` を参照。不明な値をゼロと表示しないでください。
- **コマンド・通知**: `.commands` で一覧を取得した後 `suggestions(_:text:)`。`builtin(_:_:)` に表示時の `RestoredDraft` を渡します。コマンドの受付とセッション操作の完了は別です。`receipt` だけが開始した Host・Pi に結び付いた結果で、`.unknown` は成功ではありません。閉じる操作は `dismissReceipt(host:_:)`。分岐・ツリーの返却本文は同じ Pi の宛先セッションが ready になった時、その空の入力欄に一度だけ反映します。
- **Chat-Edge**: `edge` の connection・phase / unconfirmed / pcPrompt / attachCommand、`move` の from・to と `local`。`needsMoveConfirmation` のときは前の会話に下書きを保持、入力欄の「新しい送信先を確認」から `confirmMove(_:id:)` で今回だけ確認（端末の操作元は推定しない）。`attachCommand` が nil ならコピー操作を出さないこと。
- 画面は `Chat.swift`・`Composer.swift`・`Markdown.swift`・`Transcript.swift`・`Question.swift` と新規 View を変更可。`SessionModel.swift`・`Rows.swift`・`ChatSheet.swift`・`ChatImages.swift`・`ChatNotices.swift`・`Photo.swift`・Keychain・Rust/FFI と生成物は維持してください。実機・見た目・スクリーンショット比較は後半の範囲です。

### デスクトップ配布版（macOS・Linux）

0.3.0 の配布対象は Linux のみです。macOS は署名・公証待ちで、CI の zip は公開版ではありません。版の正本は `apps/desktop/Cargo.toml` の `package.version`、タグは `desktop-vX.Y.Z` です。設定では公開版と比較できます。自動で更新はしません。

- **macOS:** `PiMate-Desktop-<版>-macos-{arm64,x86_64}.zip`。Apple Silicon / Intel ともネイティブ runner で Metal を含めてビルドし、不要なもう一方のバイナリを配らないため universal にはしません。zip を展開し `PiMate Desktop.app` を「アプリケーション」へ。開発版 `PiMate RS.app` を置換せず LaunchServices が区別できる名前と ID（配布版 `dev.pimate.desktop`、開発版 `dev.pimate.rs.desktop`）です。バンドルを区別しても通常起動の登録・設定の保存先は共通です。
- **署名の確認:** ad-hoc は改変検出用で発行元の本人確認にはなりません。ブラウザから取得した未公証の `.app` は Gatekeeper に拒否される場合があります。出所と成果物を確認し、信頼できる場合だけ、一度開いて警告を閉じ「システム設定 → プライバシーとセキュリティ → このまま開く」で**そのアプリだけ**許可します（[Apple の手順](https://support.apple.com/102445)）。Gatekeeper 自体を切らず、隔離属性を一括削除しません。
- **Linux:** `PiMate-Desktop-<版>-linux-{x86_64,aarch64}.tar.gz`。Ubuntu 24.04 同世代の glibc、Wayland または X11/XCB、xkbcommon（x11）、fontconfig、OpenSSL 3、Vulkan ローダーと対応 GPU ドライバ（Mesa 等）、ユーザーセッションの D-Bus と Secret Service の解除済み既定コレクション（GNOME Keyring 等）が必要です。展開後 `bin/pimate-desktop` で起動できます。アプリ一覧にも登録する場合は展開先の `bin/`・`share/` の内容を `~/.local/bin/`・`~/.local/share/` に同じ構造で配置し、デスクトップセッションの PATH に `~/.local/bin` を含めてください。

[Desktop packages](.github/workflows/desktop-package.yml) は PR・main の対象変更で4種類の release archive を Actions artifact に残します。ローカルで組み立てるには同じ OS/CPU でリポジトリ直下から `cargo build --release --locked -p pimate-desktop && apps/desktop/package.sh`（要 `jq`・`cargo-about` 0.9.3・Rust の標準ライブラリのライセンス文書）。MIT LICENSE、依存 crate・標準ライブラリの notices も収録し `dist/` に出力します。

**両 OS を配布する場合の署名・公開手順:** 下記の `publish=true` は macOS も添付するため、Linux のみの 0.3.0 には使いません。

1. GitHub Environment `desktop-release` の deployment branch policy を branch `main` のみ・tag なしに設定済みです。公開前にもこの制限を確認してください。可能なら承認者も設定します。Apple Developer Program で **Developer ID Application** 証明書と秘密鍵を用意し、パスワード付き PKCS#12 にエクスポート。公証用に App Store Connect のチーム API キー・Key ID・Issuer ID を用意します。
2. `desktop-release` の **Environment secrets のみ**に `DEVELOPER_ID_P12_BASE64`（p12 の base64）、`DEVELOPER_ID_PASSWORD`、`ASC_PRIVATE_KEY_BASE64`（p8 の base64）、`ASC_KEY_ID`、`ASC_ISSUER_ID` を設定します。repository・organization secret や動作確認 VM には置きません。5個すべて未設定なら ad-hoc のまま、一部だけなら失敗。設定済みなら公開時に一時 keychain で hardened runtime・タイムスタンプ付き署名 → `notarytool submit --wait` → staple と検証を経て zip を作り直します。
3. 公開承認後、main に統合された対象 commit の既存 `desktop-v<版>` タグを指定し、Actions の **Use workflow from: main**・`publish=false`（既定）で dry-run。タグ名・main の履歴・Cargo 版を照合した SHA からビルドします。タグ push は公開を開始しません。検証後だけ `publish=true`。公開直前に live tag・ビルド SHA・最終アーカイブ内の版を再照合します。macOS は Info.plist の両版、Linux は native runner の `--version` と tar.gz 内バイナリの SHA-256 を照合し、公開権限のあるジョブではバイナリを起動しません。同一実行の4成果物を添付し、Release 本文に macOS の署名・公証状態を書きます。公開済みファイルは上書きしません。

Linux の配布物の接続試験は、tar.gz を展開し、`cargo build --locked -p pimate-host` 後に `apps/desktop/tests/linux-session.sh package /tmp/pi-package /展開先/bin/pimate-desktop`。模擬 Host は debug ビルド、起動するアプリには必ず展開したバイナリを指定します。隔離 GNOME の必要ツールは次節を参照してください。

### デスクトップの操作

左でマシンと Pi を選び、中央で会話を読み書きします。右には変更・サブエージェント・GitHub を開けます（幅 1080px 未満では会話に重ねます）。差分の行を選ぶと下書きに引用されます。モデル・Thinking と使用状況は入力欄の下、セッション操作は会話右上の ⋯ にあります。

| キー（macOS は Ctrl を ⌘ に読み替え） | 操作 |
|---|---|
| Enter / Shift+Enter | 送信／改行。日本語変換中の Enter は確定のみ。設定で Ctrl+Enter 送信に変更可 |
| Ctrl+K / Ctrl+N | Pi・コマンドを検索／新しい Pi |
| Esc | 手前のメニューやパネルを閉じる。会話で実行中なら停止 |
| F6 / Ctrl+L / Ctrl+B | 次のペイン／入力欄へ／サイドバーの表示切替 |
| Ctrl+↑・↓ / Ctrl+1〜9 / Ctrl+Shift+A | 前後の Pi／番号で選択／次の要対応 |
| Ctrl+Shift+M・D・S・G | モデル・Thinking／変更／サブエージェント／GitHub |
| Page Up・Down / Ctrl+End | 会話をスクロール／末尾へ |
| Ctrl++・−・0 | 文字を大きく／小さく／標準に戻す |
| Ctrl+, / Ctrl+/ | 設定／ショートカット一覧 |

設定は Linux では `$XDG_CONFIG_HOME/pimate-rs-desktop/`（既定 `~/.config/pimate-rs-desktop/`）、macOS では `~/Library/Application Support/pimate-rs-desktop/` にあります。`settings.json` は表示と質問ツールの既定、`layout.json` はサイドバーの幅、`ctrl-enter.json` は送信キーです。接続用の資格情報はこれらに含みません。

**既知の制限:** X11 で画像を添付するときは貼り付けではなくドラッグ＆ドロップを使ってください。OS 通知はなく、要対応の件数はタイトルとサイドバーで確認します。スクリーンリーダーには対応しません。セッション名の変更と、送信済みの割り込みの個別取消はできません。Linux で終了や削除の確認を開くと、最初はキャンセルが選ばれています。開いたときの Enter を押し続けても実行されません。キーボードで実行するには、いったんキーを離し、Tab で実行ボタンを選んでから Enter を押してください。

### デスクトップ開発版（macOS・Linux）

Xcode（Metal Toolchain を含む）と Rust stable で `apps/desktop/run-macos.sh`。開発用 `.app` をビルド先に作り LaunchServices から起動します。ssh から裸の実行ファイルを起動するとウィンドウは出ても日本語 IME が ASCII 入力になったためです。`apps/desktop` は FFI を使わず、中核と Markdown の Rust API を直接呼びます。`リンクを貼り付ける` → 表示されたマシン名・アドレスを確認 → 登録。左の Pi を選択して会話を開きます。入力待ちはマシンをまたいで「要対応」に集め、各 Pi の起動元を併記します。既定は **Enter で送信、Shift+Enter で改行**。日本語変換中の Enter は確定だけです。設定 → キーボードで ⌘↩（macOS）／Ctrl+Enter（Linux）送信にも戻せます。作業中は「割り込む」、質問・PC 操作の待機中は送信不可ですが、実行中のターンは停止ボタンまたは Esc で止められます。Ctrl+K（macOS は ⌘K）またはサイドバーの検索から Pi・コマンド・アプリ操作を検索できます。表示は OS の明暗に追従し、文字サイズを3段階で変更できます。

- [Linux 試作の結果（#2）](https://github.com/phni3j9a/pimate-rs/issues/2) と同じ `gpui 0.2.2` と型互換な `gpui-component 0.5.1` の複数行 `InputState` を使用。本文は `pimate-markdown` を UI 外で解析し、HTML を経由せず段落・表・コード等を GPUI で描きます。可変高の会話リストは表示範囲とその前後だけを描画。逐次表示中も段落やセル内をドラッグで部分選択し、⌘C / Ctrl+C でコピーできます。リンクは HTTP(S) のみ。Tab でリンクのある段落へ移動し、左右キーでリンクを選び Enter でも開けます。画像やローカルファイルは自動取得しません。Linux の xattr 0.2.3 は削除された libc の ENOATTR 別名を ENODATA に置換（`vendor/xattr`）。Wayland の入力機器が変わったときの二重確定の修正は `vendor/gpui/PATCH.md` を参照。
- macOS の `store.rs` は登録をメタデータ・token 一体の Keychain 項目に格納します。アプリの複数起動時もファイルロックで置換と条件付き失効を直列化します。ロックファイルに資格情報は書きません。`cargo test --locked -p pimate-desktop` は毎回固有の `.test.*` サービスで試験。模擬 Host（前述の `scripts/android-demo.py`）を試すときは `apps/desktop/run-macos.sh --demo`（先に起動中のアプリを終了）で別サービスを使い、使い終えたらアプリ終了後 `security delete-generic-password -s dev.pimate.rs.desktop.demo -a registrations` で模擬登録だけ削除できます。
- Linux は `cargo run --locked -p pimate-desktop`（開発用の模擬登録は末尾に `-- --demo`）。セッションの Secret Service と解除済みの既定コレクションが必要です。GPUI が既に使う `oo7` の暗号化 D-Bus セッションに直接保存するため、別の依存ツリーやファイルへのフォールバックはありません。macOS と同じ条件付き更新・登録形式を使い、読み取りから置換まで同じコレクションを使います。登録一覧と接続準備は画面スレッドの外で処理し、ロック中・既定コレクションなし・サービス利用不可・書き込み発行前の D-Bus 読み取りの5秒の期限超過は Storage エラーです。登録・保存の確認中も画面は操作できます。`CreateItem` を発行した後は返信まで編集ロックを保ち、5秒で失敗とはみなしません。通信断などで保存結果が不明なら、自動再送せず登録一覧を読み直します。サービスの起動・解除後に「登録を読み直す」で再試行してください。
- Linux の画面を自動で確かめるには `cargo build --locked -p pimate-host -p pimate-desktop` 後、`apps/desktop/tests/linux-session.sh check /tmp/pi-check` と `apps/desktop/tests/linux-session.sh hotplug /tmp/pi-hotplug`。GNOME Shell（Wayland）・IBus/Mozc・gnome-keyring・wl-clipboard・tesseract（英語）・python3-dbus・python3-pil・PID/user namespace が必要です。各回の一時 HOME・D-Bus・仮想モニターで動かし、縮小 WebP のみを指定先に残します。どちらも 50 ms の連続差分中に見出しの一部を選択し、別の模擬クライアントが割り込んだ後、応答が履歴に変わっても同じ文字列だけをコピーできることを確認します。`check` は応答しない keyring を待つ間も画面を操作できること、`hotplug` は変換中のキーボード喪失とマウス追加を検査し、送信回数は模擬 RC の受信時点で照合します。実機でのスリープ復帰・物理入力機器の着脱・X11 の IME は未検証です。
- 画面を撮るには `SESSION_SECONDS=900 DEMO_SCREENS=list apps/desktop/tests/linux-session.sh shots &` で同じ隔離 GNOME を起動します。`shots` は毎回、自分だけの新しいディレクトリ（`mktemp -d`、権限 700）を作って最初の行にそのパスを表示し、命令の FIFO・ログ・画像をすべてそこに置きます（既存のディレクトリは受け付けません）。表示されたディレクトリの `cmd` に `tests/linux-shots.py` の命令（`click X Y`・`key ctrl+k`・`snap 名前` など）を書くと、結果は `ack`、画像は同じ場所に PNG で残ります。ドライバーが起動できなければ `shots` は失敗で終わります。
- 画面担当は `screen.rs` と `markdown.rs` を変更できます。`model.rs` の revision と `main.rs` の接続・編集状態は維持してください。Host ごとの配送スレッド → 有界チャネル → GPUI Entity（UI スレッド）。過負荷・大きな差分は snapshot、再登録前の配送は接続の同一性で破棄。編集本文は Host・Pi・session（null 可）別でメモリだけに保持し、送信時のキーを渡します。RC の受付と履歴は別で、結果不明を成功扱いせず自動再送しません。未確認の復元は対象が一致し、入力が空の明示操作だけです。

ツールの出力は会話内で一度開くと、画面外へスクロールして戻っても開いたままです。送信中・割り込み待ち・未確認の本文が長いときは、入力欄の上の待ち一覧をスクロールしてください。逐次の追記で選択範囲より前の表示が変わらなければ選択を保ち、構文の確定などで前の表示が変わったときは、別の箇所を推測せずに選択を解除します。

中心画面の比較と実行記録は [PR #60](https://github.com/phni3j9a/pimate-rs/pull/60)。操作には文字ラベルを使いますが、GPUI 0.2.2 は画面要素を OS のアクセシビリティツリーに公開しません。VoiceOver / Orca による操作は未対応です。変更・サブエージェント・GitHub は会話と見比べられる右ペインに開きます（1080px 未満は重ねて表示）。差分の行を押すと下書きに引用できます。モデル・Thinking と使用状況は入力欄の下のボタンから、セッション操作は会話右上の ⋯ から開きます。`/resume`・`/tree`・`/fork` は選択ダイアログです。停止や終了の確認はキャンセルが既定です。表・画像は会話内から開きます。添付は Host の上限まで（最大4枚）。質問への回答と子への指示は通常のメッセージと区別され、`/` 候補の選択・レビュー参照の挿入は自動送信しません。

変換中は、確定済みの前半があっても送信できません。送信後の ACK は別の接続世代でも送信元の未変更の下書きだけを片付け、追加入力や変換中の編集は捨てません。検証には `PIMATE_DEMO_DESKTOP_REVIEW=1 scripts/android-demo.py`（模擬 RC だけを 50 ms ごと4文字の差分・送信 ACK は3秒待機に変更）を使います。`/demo/android` を選び、終了を待たずに表・本文が成長することを確認。確定済み本文に `nihongo` を変換中のまま追加して送信ボタンを押しても送られず、確定して送信した直後に追加の語を変換しても、受付後まで入力欄に残ります。

「登録を読み直す」は削除・失効・置換された接続を閉じ、接続できなかったマシンにつなぎ直します。入口は設定のマシン一覧（常設）、サイドバーの接続できないマシンの「つなぎ直す」、登録を読み出せないときの知らせ、マシンがないときの最初の画面です。サイドバーの幅は、設定（`settings.json`）とは別の `layout.json` に覚えます。PC 側から同じ Pi のセッションが移動したときは、前のセッションに編集本文がある場合、表示された新しい送信先を確認するまで送信できません。購読失敗時は同じ Pi の選び直しで再試行します。

X11 ではクリップボードからの画像貼り付けに対応しません。画像は入力欄にドラッグ＆ドロップしてください。Wayland と macOS では画像の貼り付けも使えます。

#### デスクトップ画面用の状態 API（#58・designer への申し送り）

`apps/desktop/src/state/` は状態担当、`screen.rs`・`markdown.rs`・新規の画面は designer 担当。以下は画面を接続するための API で、上記の確認用画面がすべての操作を提供しているわけではありません。画面を開くとき一度だけ `Desktop::open_chat()` または `open_machine(host_id)` の `Page` を保持します。再描画のたびに取り直さず、閉じる・対象変更時は `state.close_page()` と画面の Task の取消。開き直しは別世代です。操作結果とエラーの描画直前に `page.visible(app)` を確認。切断／タイムアウト／`OutcomeUnknown` は成功でも再送の指示でもありません。

| 画面 | 読み取り・操作 |
| --- | --- |
| 質問・入力 | `rows.group("question")` の表示した ID と JSON を `page.question(id,shown)` に渡し、`Question.fields` を編集後 `answer()`（選択肢を事前選択しない）。送信ボタンは `prepare_submission(&page,w,cx)` → `submit().await` → 受付済みの場合だけ `finish(app,w,cx)`。通常本文・画像・組み込みを同じ IME／未確認移動のゲートに通す。`finish` が返す `model/resume/fork/tree` はシートを開く指示、送信ではない。`restore_draft` は空欄への明示復元だけ。割り込みは `page.abort()`。 |
| 添付・画像 | OS ピッカーを出す前に `start_picker(&page)`、背景で `picker.encode(&paths)`（最大4枚、JPEG に逐次縮小、各512 KiB・計600 KiB）、画面スレッドで `finish_picker(picker, images)`。取消なら picker を drop。`state.attachments.images(key)`・`remove(page,shown,index)`。履歴は `state::images::{history_count,history_source}` で元の位置を指定、Markdown は可視タイルだけ `Source::markdown(image,answer_id)` → `page.image(source,thumbnail)`（背景 Task）。返るのは圧縮済みバイト。PC ファイルを直接開かず、回答 ID 別にキャッシュ。HTTPS のみ（無資格情報、転送先も検証、5回・20秒・8 MiB）、HTTP は明示リンク。描画用復号も寸法制限を付けること。 |
| コマンド・シート | `open_sheet(SheetPage)`／`active_sheet()`。`Sheet::begin(action)` → `run().await.finish(app)`（`loading()`・`reply`・`error`）。`SheetAction::{Commands,Models,Select,Favorites,Default,Thinking,Sessions,Tree,New,Resume,Fork,Move,Label,Compact,Reload}`。Commands 後 `page.suggestions(text)` は候補のみ。`model/session/node` 行、`view/defaultModel`・`view/context`、`page.usage(now_ms)` の stats・allowances を表示し、未知を 0 と扱わない。終了は `page.shutdown_confirmation()` を確認後 `confirm()`。`attach_command(view/abducoSession)` が Some のときだけコピー。 |
| 送信先変更・完了 | `pending_move_with` は本文**または画像**の下書きを照合。`confirm_move` は表示した移動 ID を渡す。`Operation` の ACK は完了とは別。Host の `rows.receipts.shown(pi,started_at)`・`outcome()` は観測済み要求のみ。`dismiss(pi,id)`、ready な入力へ `receive_editor_text(w,cx)` で分岐／ツリーの確定本文を一度だけ反映。`draft` 行の未確認、`view/pcPrompt` は履歴と区別。 |
| 変更レビュー・GitHub | `page.review(turn)` を背景で計算（None は読み込み済み全体）。`files[].pieces` は呼出順、`Patch::parse` は hunk・行番号・追加削除・末尾改行、差分未提供時は `omitted`。`review.ask_about(app,w,cx)` は既存下書きに参照を追記するだけ。`page.github(GitHubQuery)` の一覧／詳細・省略フラグ、`page.github_item(item)` の検証済み `url`・`ask_about`。失敗を空一覧と扱わない。 |
| サブエージェント | `rows.subagent_counts()`・`group("run")`・`original_run(tool_call_id)`。開いた子の `page.work_record(Child)` → `load().await`（不透明 cursor、1回8ページ・最大2048行、`more`／`truncated`）。`page.stop(run,viewed_child)` の `scope()` を表示し、確認後にだけ `confirm()`（`interrupt`／`abort_parent` は親・全体に影響）。`page.instruct(child,Instruction,text)` は対応 action だけ、成功した `Sent` を `Desktop::accept_instruction` へ。Host の差分適用時に対応を一度だけ固定し、`state.instructions.resumed(&page,source,new_run)` で移動先の run/child を一緒に選択、`for_child` で再開元と先のラベルを参照（128件・64 KiB）。 |
| 新しい Pi・マシン | マシンページの `query(MachineQuery::{Folders,Dirs,UpdateCheck,GithubStatus,Devices})` と `host_models()`（最大3 Pi・購読変更なし）。`spawn_form(settings.ask_tool)` → `folder(path,trusted)`、`spawn()` は1回のみ。`Spawned::status` は Waiting／Ready／MessageUnconfirmed／NotListed（20秒）、Ready だけ選択。`confirmation(Maintenance::{UpdateLatest,Restart,Revoke})` の `warning()` を表示、確認後 `confirm()`。更新先は実行時の最新版。マシン削除は接続中で `pairedDevices` があれば `core.revoke_and_forget(conn,id,device)`、成功時だけ `removed(host,cx)`。`OutcomeUnknown`（送信後に応答を確認できない）は失敗と断定せず PC の端末一覧での確認を案内する。結果不明・未接続・非対応・取消失敗は利用者の明示確認後だけ `forget_device(id,device)` でローカル登録を削除する。 |
| ログイン | `page.login().await` を画面の Task で保持し、切断・閉じるときに drop。`login.instance()` の `provider/auth` 行、必要に応じ `request(AuthAction::{List,Begin,Respond,Cancel})` と `follow_once()`。ログアウトは `logout_confirmation(provider)` → 確認後 `confirm()`。再接続では新しい page と hold で**一覧だけ**取り直す。`CredentialOutcome::from_flow` の `StorageUnknown` は一覧再確認、`NeedsReload` は保存済み。入力や応答をログ・設定に入れない。 |
| 設定・初回案内・登録通知 | `state.settings` の `appearance/font_size/ask_tool` のみ `Settings::path()` と `load/save` で読み書き。起動時に読み出して画面のテーマ・文字サイズに適用、失敗時は通知。初回登録成功後 `state.registration.paired_host.take()`、マシンの `rows.first_step()`（Loading／Offline／Empty／List）。登録中の `pairing` と `state.registration.message()` は、チャットの `notice` と**別の欄**に同時表示。保存不明後に一覧の読み出しも失敗したときは両方の事実を表示する。 |

登録の知らせはサイドバー、送信結果はチャット欄に表示し、別の操作の完了で上書きしません。接続準備の失敗はマシンごとに表示し、再読み込みで接続できたマシンの失敗だけを取り下げます。

画面から中核・FFI を変更する必要はありません。送信直前の snapshot 照合と中核の受付は原子的ではないので、未配達の再起動・同一 ID の再使用まで完全に排除するサーバー保証ではありません。

### iOS の残りの画面で使う状態（#48 前半）

画面を開いた時のマシン ID／`EditorKey` を捕捉し、画面を閉じたら Task をキャンセルします。操作の失敗や切断は空一覧・成功に変換せず、書き込みを自動再送しません。

| 画面 | 状態と操作 |
| --- | --- |
| 変更レビュー | `await model.review(key, turn: userEntryID)`、セッション全体は `nil`。読み込み済み履歴から、開始したターンに属する edit/write を順に集計。`files[].pieces` に元の patch、行番号・件数・末尾改行、作成根拠、差分なしの理由。`askAbout(review)` で元のチャットの下書きへ追記（画像を残し送信しない）。 |
| サブエージェント | `rows.subagentCounts`・`runs`。ツール行は `originalRun(toolCallId:)`、詳細は `workRecord(key, AgentChild(...))` を開いた子ごとに生成。`load()` は8ページまで、`more` なら続き、戻り値 true は取得中に更新あり。指示は子の `actions` にある場合だけ `control(key, child, action:text:)`。送信者のヒントは `instructions.forChild(key, child)`。resume 前の子と index を保持し、返った `newRunId` で `model.resumedChild(key, from: source, run: newRunId, index: index)` が **非 nil になった時に一度だけ run と子を一緒に**切り替える。受付済みの再開は指示の履歴（最大128件）に保持し、同じ Host・会話・Pi の起動時刻が一致する状態で一度だけ子 ID を対応付ける。詳細を閉じたり別の Pi を開いたりしても、以後の並べ替えでラベルを移さない。セッション ID がない場合は同一の表示世代内に限る。停止・一時停止は `rows.stopTarget(_:run:viewedChild:)` を確認時に固定し、現在の表示対象と一致する間だけ `control(approved)`。run 全体／親が止まる場合は影響範囲を明記。 |
| 新しい Pi | Host ごとに `SpawnForm(host:askTool:)` を作成。`machineRequest(id,.folders/.dirs(path:))` の結果から `folder(_:knownTrusted:)`。入力中は対応 PC の子フォルダを前方一致で最大8件補完し、同じフォルダの末尾 `/` は信頼照合時だけ無視する。送信するパスの空白や Unicode は書き換えない。質問ツールの切り替えは次回の既定にも保存する。`hostModels(id)` は未購読でも最大3つの Pi に照会。選択画面では PC の既定モデル・Thinking、お気に入りと全モデルを表示し、7件以上なら検索できる。Pi がない場合は PC の既定モデルを使える。`spawn(form)` の `.ready` のみその Host で select。`.messageUnconfirmed` は起動済みで初回本文だけ未確認、`.notListed` は起動済みで一覧未到着（20秒）。 |
| マシン | 接続・能力は `rows[id].groups["host"]`。`machineRequest(id,.updateCheck/.githubStatus/.devices)`。更新・再起動・端末取消・登録削除は `confirmation(host:action)` と `warning` を表示し、確認した値を `confirm(_:)` に渡す。更新先は**実行時の最新版**、結果は Host の返した版。マシン削除では接続中なら先に PC で自端末の登録を取り消し、未接続・非対応・取消失敗なら PC 側に登録が残る旨を表示して手元だけ消すか確認する。マシン詳細からオフライン時の再接続・認証失効時の再ペアリング、gh 未ログイン時の `gh auth login` のコピーも可能。 |
| プロバイダー | `ProviderState(host:)` を作り、`.task(id: rows.connectionGeneration)` 内で `hold(model,generation:)`。接続世代の変化で専用 Pi を解放・再取得し、一覧も取り直す。`page.login` の `target.instance` で `provider`／`auth` 行を参照。入力 UI も世代で破棄。`login.request(.authBegin/.authRespond/.authCancel/.authLogout, fields)`、対象プロバイダーを固定してログアウトを確認。状態更新後は `.authList`、working/input/saving なら `followOnce()` をループ（Rust が接続・専用 Pi ごとの同時 wait を拒否）。`CredentialOutcome.storageUnknown` は一覧再確認、`savedNeedsReload` は保存済み・Pi 再読込。中核が検証した authorization のみ明示タップで開く。API キー・コードは入力時だけ使い、復元・ログ・画面撮影の対象外。 |
| GitHub | `github(key,query:)` は `kind=list` または `kind=issue/pr,number`。`data` に一覧／詳細・省略フラグ。`GithubStatus` は CLI なし・未ログイン・未確認を区別する。`GithubItem(key,item)` の検証済み URL だけを開き、`askAbout(item)` は元の下書きに追記するだけ。 |
| 設定・初回案内 | `PiSettings` の `appearance/typeSize/askTool`（表示は既存と同じ AppStorage）、`version`・`issueURL`。`AppUpdate.check()` はインストール中の bundle ID で App Store の公開版を照会。未配布・通信失敗は `.unknown`（Android の APK や PC のリリースは比較しない）。`pairedHostId` は初回の成功後のみ。案内先を取り出したら `consumePairedHost()`。`rows[id].firstStep` は loading/offline/empty/list。初期一覧到着前を empty と扱わない。 |

画面担当は新規 View、`Probe.swift`・`Pair.swift`・`Drawer.swift`・チャットの View を変更可。状態は `SessionModel.swift`・`Rows.swift`・`ChangeReview.swift`・`SubagentState.swift`・`MachineState.swift`・`PiSettings.swift` を使用し、Keychain・Rust／FFI・生成物は変更不要です。実機・見た目・モックとの比較は画面の PR で扱います。
