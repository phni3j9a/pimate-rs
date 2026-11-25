crates.io `gpui 0.2.2` の Wayland 修正。`gpui-text-input.patch` は [zed#46560](https://github.com/zed-industries/zed/pull/46560)（d92f477）の試作バックポートで、二重確定の原因だった古い text_input を破棄して作り直す。

本 vendor はその対策を拡張し、同じ seat の keyboard 能力が継続する通知では keyboard と text_input の双方を再利用する。マウスを追加しただけで進行中の変換を取り消さない。keyboard 能力を失うか seat を交換する際には解放し、入力欄の marked range も取り消す。喪失時は Shift などの押下状態も解除する。登録された各 seat の keyboard 能力を記録し、列挙順が最後の seat に限定せず、現在の keyboard seat が使える限り維持してから別の候補に切り替える。ポインター・クリップボードを含む全面的なマルチ seat 対応ではない。古いオブジェクトからの遅着イベントは無視する。macOS のコードは変更しない。

修正済みの公式 gpui と型互換な gpui-component が公開され、Wayland の変換中の機器追加と macOS の IME/ACK 試験が通ったら vendor を外す。
