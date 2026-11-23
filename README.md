# pimate-rs

[PiMate](https://github.com/phni3j9a/PiMate)（PC で動く [Pi Coding Agent](https://github.com/earendil-works/pi) をスマホなどから使うツール）の Rust 版です。最終的に TS 版を置き換えます。

**状態:** 方針の検討中。まだコードはありません。

## 目的

- iOS とデスクトップを含めて、アプリを同じ作りにまとめる
- Host を単一バイナリにして、Node や abduco への依存を減らす

## 決まったこと

- アプリは iOS / Android / Linux / macOS の4つの OS に対応する
- 通知は当面作らない
- Pi の中で動く Remote Control Extension は Pi のプロセス内で動く TS なので、TS のまま残す
- iOS / macOS のビルドと確認は Devin Cloud の macOS VM で行う（手順は TS 版の [docs/devin-macos.md](https://github.com/phni3j9a/PiMate/blob/main/docs/devin-macos.md)）

## 検討中

- Host も Rust にするか
- UI の作り方
  - A. Rust で中核（通信・状態・プロトコル）を作り、UI は各 OS 標準（SwiftUI / Jetpack Compose / Linux 用）を UniFFI などでつなぐ
  - B. UI も Rust で1本にして4つの OS で動かす

## ライセンス

[MIT License](LICENSE)
