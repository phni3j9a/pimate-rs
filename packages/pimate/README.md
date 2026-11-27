# PiMate — PC の Pi をスマホや Grok Bot から使う

TS 版（0.1.x）から切り替える場合は、作業中の Pi を先に保存・終了し、[移行手順](https://github.com/phni3j9a/pimate-rs/blob/main/README.md#npm-配布と-ts-版からの切り替えm8-で公開を承認した後)に従ってください。Rust 版から旧端末には再接続できません。

Pi のターミナルで `pi install npm:@phni3j9a/pimate`（更新は `pi update npm:@phni3j9a/pimate`）を実行します。Pi を再起動し、`/pimate setup` で Host と CLI を設定してください。

## Grok Bot の設定

1. [PiMate テンプレート](https://x.ai/bot/EL8LzPmNquZTjEawTxAI1)を Bot に追加します。
2. Bot は PC の Skill を自動で読みません。パッケージの導入・更新後、Bot に `~/.pi/agent/npm/node_modules/@phni3j9a/pimate/grok-bot/skills/` 内の4つの `SKILL.md` を読み、同じ名前・説明で全文をプライベート Skill として保存・有効化するよう依頼します。
3. `pimate-getting-started` に沿って使う PC を確認します。チャットに質問を届けたい場合は[テンプレート構成](https://github.com/phni3j9a/pimate-rs/blob/main/packages/pimate/grok-bot/TEMPLATE.md)を参照してください。

カードの選択肢には `pimate ui ID REQUEST --label LABEL`、質問票には `--answer-labels JSON` を使います。PC 側で表示と一致する候補が1つのときだけ回答します。従来の値や `--answers` は原文のままです。

Bot が監視外の Pi を待つときは `pimate wait ID --redact` または `pimate prompt ID TEXT --wait --redact` を使います。既定は原文、`--redact` は認識できる秘密を伏せ字にします。すべての秘密を検出できるわけではありません。タイムアウトは送信失敗ではないので、指示を送り直さないでください。

[アプリの入手・PC の設定・CLI の詳細](https://github.com/phni3j9a/pimate-rs#readme)
