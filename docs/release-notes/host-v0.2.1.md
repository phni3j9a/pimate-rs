# PiMate Host・npm 0.2.1

- Grok Bot の初期設定と、質問をチャットで受け取るための4つの Skill を同梱しました。更新したら、Bot にパッケージの `grok-bot/skills/` を読み直し、プライベート Skill を更新するよう依頼してください。
- 質問カードに表示された文言で回答できます（`pimate ui` の `--label` と `--answer-labels`）。同じ表示の候補が複数あるときは、誤った回答を送らずに止まります。
- `pimate wait` と `pimate prompt --wait` は従来どおり原文を返します。Bot に結果を渡すときは `--redact` を付けると、認識できる秘密を伏せ字にできます。すべての秘密を隠せるわけではありません。
- 互換性のない Pi が一覧に出ない場合は、PC の Host のログで理由を確認できます。

更新コマンドは `pi update npm:@phni3j9a/pimate` です。
