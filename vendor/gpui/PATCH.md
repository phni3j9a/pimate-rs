crates.io `gpui 0.2.2` の Wayland 修正。`gpui-text-input.patch` は [zed#46560](https://github.com/zed-industries/zed/pull/46560)（d92f477）の試作バックポートで、二重確定の原因だった古い text_input を破棄して作り直す。

本 vendor はその対策を拡張し、同じ seat の keyboard 能力が継続する通知では keyboard と text_input の双方を再利用する。マウスを追加しただけで進行中の変換を取り消さない。keyboard 能力を失うか seat を交換する際には解放し、入力欄の marked range も取り消す。喪失時は Shift などの押下状態も解除する。登録された各 seat の keyboard 能力を記録し、列挙順が最後の seat に限定せず、現在の keyboard seat が使える限り維持してから別の候補に切り替える。ポインター・クリップボードを含む全面的なマルチ seat 対応ではない。古いオブジェクトからの遅着イベントは無視する。macOS のコードは変更しない。

修正済みの公式 gpui と型互換な gpui-component が公開され、Wayland の変換中の機器追加と macOS の IME/ACK 試験が通ったら vendor を外す。

## 明暗切り替えと再描画（#58）

Wayland の XDG portal の `WindowAppearance` を配送するとき、対象ウィンドウのハンドルだけ複製し `client.borrow_mut()` を解放してから通知する。選択・逐次表示中に GNOME の明暗を変えると、通知から描画・`set_cursor_style` に再入し、旧実装では `RefCell already borrowed` で終了した。`apps/desktop/tests/linux-session.sh check` / `hotplug` は独立した実 portal と仮想モニターで、この切り替えと部分選択を検査する。

独自の修正。[確認した上流の Wayland client](https://github.com/zed-industries/zed/blob/2664a0aa1eae5dabf8f8273c03f88d53061ca7dc/crates/gpui_linux/src/linux/wayland/client.rs#L1162-L1184) でも appearance を借用中に配送しており、同じ修正の PR は見つからなかった。

既知の同種の危険: 上流の隣接する `ButtonLayout` も借用中にウィンドウへ通知する（この vendor にはそのイベントなし）。この版の同じ portal の他の通知はカーソルのテーマ・サイズだけで、ウィンドウへのコールバックは発火しない。keyboard のフォーカス・レイアウト変更、pointer の enter/leave は配送前に既に借用を解放する。今回はこれらを変更していない。
