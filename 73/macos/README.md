# デスクトップ 0.3.0：macOS 確認（#73）

Devin VM 1、macOS 26.6.2 (arm64)。[CI run 45951303095](https://github.com/phni3j9a/pimate-rs/actions/runs/45951303095) の `PiMate-Desktop-0.3.0-macos-arm64.zip` を起動。同じ VM の `scripts/android-demo.py` と Rust Host をループバックで接続。資格情報とテストデータは一時領域のみ。撮影は VNC。比較は左が `73/mock/`、右がアプリ（タイトルバーを含むウィンドウの切り抜き）。ダークはアプリの設定で選択。モックの会話内容や設定カテゴリと完全に同じデータを再現する試験ではない。

| 画面 | ライト | ダーク |
|---|---|---|
| 初回案内 | [画像](First.png) | [画像](First-dark.png) |
| ペアリングの確認 | [画像](Pair.png) | [画像](Pair-dark.png) |
| サイドバー | [画像](Sidebar.png) | [画像](Sidebar-dark.png) |
| 会話 | [画像](Main.png) | [画像](Main-dark.png) |
| 変更インスペクター | [画像](Review.png) | [画像](Review-dark.png) |
| ⌘K | [画像](Palette.png) | [画像](Palette-dark.png) |
| 設定 | [画像](Settings.png) | [画像](Settings-dark.png) |
| 新しい Pi | [画像](New-Pi.png) | [画像](New-Pi-dark.png) |
| 質問カード | [画像](Waiting.png) | [画像](Waiting-dark.png) |

## 結果

- **新しい Pi**：標準サイズで起動ボタンのラベルがダイアログ下端に隠れる。サイズを小さくするとボタン自体が見えない。フォーム内で下にスクロールしても回復しなかった。[大小比較](New-Pi-clipped.png)。起動 API のエラーと判断したものではない。
- [操作メニュー](Menu.png)の「サイドバーを表示 / 非表示」で実際に切り替え。⌘K でパレット、Esc で閉じ、⌘N で新しい Pi。macOS のタイトルに選択中の Pi 名が表示され、ドラッグで移動、下端でリサイズ、黄色ボタンで最小化後にアプリを再度開いて復帰した。
- [日本語 IME](IME.png)：`devin-mac input-source` で日本語にし、`devin-mac key nihongo` と `:return`。変換候補が消えて「日本語」が入力欄に残り、模擬 Host の受信は増えない。「Enter で送信」設定で別の会話でも同じ操作を繰り返し、受信行数は確定で 1→1、次の Return で 1→2（`["images","日本語"]`）。
- [画像の貼り付け](Paste.png)：NSPasteboard に置いた赤い画像を ⌘V、添付数 0/4→1/4。送信ボタンを押すと会話中に画像 1/1 が表示された。
- 判定範囲は配布物の VM 内での表示・操作。公証、Intel Mac、物理デバイスの入力遅延、実際の Pi を起動しての疎通は対象外。
