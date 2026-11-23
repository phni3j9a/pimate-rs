# Rust で 4OS（iOS / Android / Linux / macOS）クライアントを作る UI 方式の調査

## 決定（2026-11-23）

この調査のあとで、次のように決めた。下の「推奨」は、この決定より前に書いたもの。

- **WebView を使う方式は使わない。** アプリの質が下がるため（利用者の判断）。これで方式 B の候補だった Dioxus の WebView 描画は外れ、Tauri も対象外になる。
- **Discord の新クライアント Phoenix（2026-11-04 に CTO が発表）と同じ形にする。** Phoenix は、スマホを各 OS の標準 UI ＋ 共有の Rust 中核、デスクトップを GPUI で作り直している。CTO の言葉は "mobile keeps native UI but shares app code with desktop via Rust … it's not Tauri, it's GPUI"（[InfoQ 2026-11-19](https://www.infoq.com/news/2026/11/discord-rust-rewrite-gpui-zed/)、[The Verge 2026-11-05](https://www.theverge.com/news/1027379/discord-ditching-electron-web-app)）。スマホ側の UI が SwiftUI か UIKit かは公表されていない。
- **構成**: iOS は SwiftUI、Android は Jetpack Compose（どちらも UniFFI で Rust の中核とつなぐ）。macOS / Linux は GPUI。
- **Linux の条件**: 下の表のとおり GPUI は Linux の IME に不具合の報告がある。開発機（Wayland + IBus + Mozc）で確かめ、通らなければ Linux だけ GTK4（relm4）にする（[#2](https://github.com/phni3j9a/pimate-rs/issues/2)）。

---

調査日: 2026-11 下旬（取得できた一次情報は 2026-11-18 頃まで）。
表記: **[事実]** = 一次情報（公式ドキュメント・リリースノート・GitHub）で確認したもの / **[二次]** = 二次情報やまとめサイトだけで確認したもの / **[推測]** = 調査者の推論。

## 結論（先に）

- **方式 B で実用ラインに最も近いのは Dioxus 0.7 の WebView レンダラ**（stable 0.7.10、0.8 は alpha）。UI は Rust（RSX）で書き、iOS は WKWebView、Android は System WebView、Linux は WebKitGTK に描画させる。日本語 IME・テキスト選択/コピー・VoiceOver/TalkBack・Markdown 表（HTML table）は **OS の WebView に任せられる** ため、要件との相性が一番よい。**[推測]**（WebView の機能を使う前提での推論。Dioxus 上で日本語 IME を実機確認した一次情報はない）
  - 最大のリスク: 小さなチームによる 1.0 前のフレームワークである点。モバイルのネイティブ連携（カメラ/QR、画像ピッカー、Keychain/Keystore）は、0.7.4（2026-03-28）で入った `manganis::ffi` で Swift/Kotlin を自前で書く前提で、プラグインのエコシステムはまだ薄い。さらに WebView のキーボード/viewport 周りの挙動差（Android WebView M139 以降の IME リサイズ、iOS 26 の VisualViewport 不具合）は自分たちで吸収する必要がある。
- **独自描画系（Slint / Makepad / egui / Iced / Xilem / Freya / Floem / GPUI）は、2026-11 時点ではどれも「日本語 IME + 選択 + a11y + リッチテキスト」をモバイルで揃えていない**。Makepad（Robrix が iOS/Android で alpha 配布中）と Slint（1.15〜1.19 でモバイル改善中）が次点。
- **方式 A（Rust コア + UniFFI + SwiftUI/Compose）** は、Element X（matrix-rust-sdk）という大規模な本番実績がある王道。UniFFI 0.32.x（2026-09）は async 関数、async trait、foreign trait（Swift/Kotlin 実装のコールバック）に対応する。ただし **Stream/Flow/AsyncSequence をそのまま変換する機能はない**ため、イベント購読はリスナー trait か `async fn next()` 型のポーリングで自前実装する。Linux は GTK4/libadwaita（relm4 0.11）が IME 面で最も安全。
- **推奨: 方式 A**（理由は末尾）。Linux を安く済ませたいなら、Compose Multiplatform Desktop で Android の Compose UI を Linux に流用する案も検討に値する **[推測・未検証]**。

---

## 1. 方式 B 候補の比較表

| 候補 | 最新版（日付） | iOS / Android | モバイルの日本語 IME | 選択/コピー | リッチテキスト・表 | 長リスト仮想化 | a11y（VoiceOver/TalkBack） | キーボード押し上げ | ネイティブ機能 | ライセンス | 実運用例 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **Dioxus（WebView）** | 0.7.10 stable / 0.8.0-alpha.3（2026-10-16）[事実] | 公式ドキュメントで mobile を "first-class target" と記載（WebView 描画）[事実] | OS の WebView に任せる [推測] | WebView 標準 [推測] | HTML/CSS（table 含む）[推測] | 自前（CSS / JS 仮想化）[推測] | WebView 標準 [推測] | WebView + `windowSoftInputMode` 等。WebView の viewport 問題あり [事実: Android/Apple 文書] | `manganis::ffi` で Swift/Kotlin を直接バインド（0.7.4, 2026-03-28）。permission / Info.plist / Manifest も設定可 [事実] | MIT/Apache-2.0 | FlowFlow（iOS App Store、Dioxus）[二次] |
| **Dioxus Native（Blitz）** | Blitz 0.3.0-beta.4（2026-10-16）[事実] | 0.3.0-alpha.1（2026-04-23）でモバイル対応 [事実] | composition イベントは roadmap の Backlog、関連 PR #1086/#1084 は open（2026-11 時点）[事実] | テキスト選択あり（alpha.1）[事実]。clipboard / beforeinput イベントは Backlog [事実] | table は CSS Grid で疑似実装。本格的な table layout は Backlog [事実] | 未確認 | AccessKit。"Accessible text layout and input" は Backlog [事実] | 未確認 | Dioxus と同じ | MIT/Apache-2.0 | なし |
| **Slint** | 1.19.0（2026-11-16）[事実] | iOS は 1.12（2025-06）で初対応（winit+Skia）、Android は android-activity [事実] | iOS は winit の UITextInput 依存。モバイル IME 基盤の PR #10557 は未マージのままクローズ（2026-10-20）、UITextInput の自前実装は今後の予定 [事実]。Android でキーボード再表示（#14968, 2026-11-15 open）や SwiftKey の不具合（#9240 open）[事実] | 1.19 で `StyledText` に `selectable`、Android の選択挙動を改善 [事実] | StyledText のみ。Markdown 表は自作 [推測] | ListView（ensure-row-visible 等）[事実] | AccessKit の iOS 統合は検討段階（PR #10557 のコメント）[事実] | 1.15 で virtual-keyboard 領域を公開し、フォーカス要素を自動スクロール [事実] | 自前（JNI / objc2）[推測] | Royalty-free（帰属表示）/ GPLv3 / 商用 [事実] | 組込み中心。モバイル製品は未確認 |
| **Makepad** | crates.io は 1.0.0（2025-05-13）。2.0 は git dev ブランチ（Splash、2026-02）[二次] | Robrix 1.0.0-alpha.1 が iOS（TestFlight）/ Android（APK）で配布中（初出 2026-05-05）[事実] | Robrix が「Android/iOS の IME と仮想キーボードに対応」と記載。Moly v0.2.4 は「CJK 入力の初期対応」[事実] | 未確認 | Robrix は表を含むリッチ HTML 表示と全画面画像ビューアを実装済み [事実] | PortalList [事実] | 支援技術への対応は見当たらない（issue #196 open、tree に accesskit なし）[二次] | Robrix が safe inset に対応 [事実] | 自前（Robrix は iOS の SSO をネイティブ実装）[事実] | MIT/Apache-2.0 | Robrix、Moly（alpha）|
| **egui / eframe** | 活発（2026-11 も issue 活発）[事実] | Android は公式サポート、iOS は wgpu 限定 [二次] | 2026-03 に IME を大幅改善（PR #7967、デスクトップで CJK 検証）。Android では IMEOutput の情報不足（#7941 open）、Samsung キーボードで ASCII 以外が届かない報告、ドラッグ中にキーボードがちらつく（#8884, 2026-11-13 open）[事実] | 可 | 自前（egui_commonmark 等）[推測] | 可 | AccessKit | 自前 | 自前 | MIT/Apache | モバイル製品は未確認 |
| **Iced** | — | 公式は Windows/macOS/Linux/Web のみ。モバイルは非公式（iced_mobile 等）[事実] | 未確認 | — | — | — | — | — | — | MIT | なし |
| **Xilem / Masonry** | v0.4.0（10/29）[事実] | Android のサンプルが cargo apk で動く程度。"experimental" と明記 [事実/二次] | 未確認 | — | — | — | AccessKit | — | — | Apache-2.0 | なし |
| **Freya** | 0.5.3（2026-11-18）[事実] | Android は experimental（freya-android）、iOS は記載なし [事実] | 未確認 | — | — | — | — | — | — | MIT | なし |
| **Floem** | — | 本家は desktop のみ。モバイルは非公式フォーク [二次] | — | — | — | — | — | — | — | MIT | なし |
| **GPUI** | Zed 本体と一緒に開発 | 2026-11 に upstream へ初期 iOS backend が入った（"not feature parity"）。Android は非公式ポートのみ [二次] | Linux でも fcitx5 系の IME 不具合 issue が多数（#54959 open 等）[事実] | — | — | — | 弱い [推測] | — | — | Apache-2.0 | モバイルはなし |

補足:
- AccessKit は Android アダプタ 0.9.1 と iOS アダプタ 0.2.2（基本版は 2026-05 追加）を 2026-10-25 にリリース済み [事実]。各フレームワークへの組み込みはまだ途中。
- Tauri 2（Rust + Web フロント）は「UI を Rust だけで書く」条件から外れるため比較対象外。性質は Dioxus WebView とほぼ同じで、モバイルプラグイン（カメラ、barcode、stronghold 等）は Tauri の方が厚い可能性がある [推測・未検証]。

## 2. 方式 A（Rust コア + ネイティブ UI）

- **UniFFI**: 0.32.0（2026-06-30）、0.32.2（2026-09-22）[事実]。
  - Rust の `async fn` は Swift の `async` / Kotlin の `suspend fun` に変換される。実行器は Swift/Kotlin 側のランタイムを使う。Rust 側で tokio を使う場合は別スレッドで runtime を立てる設計になる [事実: futures.html, async-overview]。
  - `#[async_trait]` を使った async trait のエクスポートと、`with_foreign`（foreign trait）で Swift/Kotlin 側がコールバックを実装する形に対応。旧 callback interface は soft deprecated [事実]。
  - **キャンセルは非対応**。cancel() などを自前で用意する [事実]。
  - **Stream / Flow / AsyncSequence の自動変換はない**（#1485 はクローズ、#2755 は open の質問）[事実]。チャットのストリーミングは、`trait EventListener { fn on_event(e) }` を foreign trait で渡すか、`async fn next_event()` を Kotlin Flow / Swift AsyncStream で包む形になる [推測: 一般的なパターン]。
  - 実績: Element X iOS（SwiftUI）/ Android（Compose）が matrix-rust-sdk + UniFFI で本番運用中 [事実]。
- **Crux（redbadger）**: crux_core 0.20.0（2026-08-06）[事実]。0.17（2026-03）で Capability API を削除、0.19（2026-06）で EffectRouter を導入と、破壊的変更が続く [事実]。Elm 型のコアと、副作用をシェル側に委ねる構成。WebSocket の常時ストリームは effect として表現できるが、0.x の API 変動に追従するコストがある [推測]。今回のように通信と状態だけを Rust に寄せる用途なら、UniFFI を直接使う方が単純 [推測]。
- **Linux のネイティブ UI**:
  - relm4 0.11.0（2026-04-08、gtk4-rs ベース、libadwaita 対応）[事実]。GTK4 は IBus/fcitx5 の IM モジュールを使うので、日本語 IME・選択・a11y（AT-SPI）は最も成熟している [推測: GTK の一般的特性。本調査で個別には検証していない]。Markdown 表示は GtkTextView か WebKitGTK の埋め込みになる [推測]。
  - GPUI: Linux の IME 不具合 issue が継続して出ている（fcitx5 / Wayland / X11）[事実]。日本語ユーザー向けの Linux UI としてはリスクが高い。
  - 代替案: Compose Multiplatform Desktop（JVM）なら Android の Compose コードを Linux に流用でき、UniFFI の Kotlin バインディングも JVM で動く [推測・未検証]。デスクトップの IME 修正は継続中 [事実: compose-multiplatform-core PR #1794/#1980/#2210, 2025]。
- **macOS**: SwiftUI を iOS と共有。UniFFI の Swift バインディングは同じものを使える [推測]。

## 3. Linux（Ubuntu）開発機 + macOS VM でのビルド注意点

- iOS のビルドと署名は、どの方式でも macOS + Xcode が必須（Dioxus / Slint の公式ドキュメントに明記）[事実]。Rust ターゲットは `aarch64-apple-ios` と `aarch64-apple-ios-sim`。Slint は Xcodegen を使う [事実]。
- 方式 A: Linux 上で Rust コア、Android（cargo-ndk + Gradle）、Linux UI まで完結する。iOS は macOS VM 上で XCFramework を作って SwiftPM で取り込む形になる [推測: Element X と同様の構成]。UniFFI のバインディング生成は Linux 上でもできるが、Swift 側のビルドは VM で行う [推測]。
- 方式 B（Dioxus）: `dx` CLI で Android は Linux 上でビルドできる（Android SDK/NDK が必要。Arch での手順記事あり、2026-08）[二次]。iOS は VM 上で `dx bundle`。Linux desktop 版は WebKitGTK に依存する [推測]。
- Mac mini 2018（Intel）を使う場合、iOS シミュレータは x86_64 になる。Dioxus 0.7.4 で Intel Mac のシミュレータ不具合が修正された [事実: PR #5268]。他のフレームワークは未確認。
- 共通: T2 Mac 上で Android エミュレータを使う場合は KVM の有無を確認する（未確認）。

## 4. 未確認事項（要実機検証）

1. Dioxus WebView 上の日本語 IME（変換中の表示・確定、Enter 送信との競合）と、iOS/Android でのキーボード押し上げ。どちらも一次情報がない。
2. Dioxus 0.8 の安定版リリース時期（2026-11 時点で alpha.3）。
3. Makepad 2.0 の crates.io 公開状況と、日本語（変換候補）での実機挙動（Robrix/Moly の記載は CJK 全般）。
4. Slint Android の日本語 IME の composition 実機挙動（Android バックエンドは独自 InputConnection だが、日本語での検証報告は見つからなかった）。
5. GPUI iOS backend の詳細（ミラーサイト経由の情報のみ）。
6. Compose Multiplatform Desktop の Linux（Wayland + fcitx5/IBus）での日本語 IME の現状。
7. Tauri 2 のモバイルプラグインを Dioxus から流用できるか。

## 5. 推奨と理由

**方式 A（Rust コア + UniFFI + SwiftUI〔iOS/macOS〕+ Compose〔Android〕+ GTK4/relm4〔Linux〕）を推奨する。**
- 必須要件（日本語 IME、部分選択/コピー、a11y、OS の文字サイズ追従、ボトムシート、カメラ/QR、画像ピッカー、セキュアストレージ）は、OS 標準 UI ならすべて最初から揃っている。方式 B の独自描画系は、2026-11 時点で IME か a11y のどちらかが未完成 [事実ベース]。
- 既存の Expo/RN Android 版の画面構成は Compose に移しやすい [推測]。
- 弱点: UI を 3 系統（SwiftUI / Compose / GTK）書く工数と、ストリーム購読のブリッジを自作する手間。
- 方式 B を選ぶなら Dioxus WebView 一択。最初に「日本語 IME + ストリーミング表示 + キーボード押し上げ」の実機 PoC を Android/iOS で 1〜2 日行い、合格したら採用、という条件付きにする [推測]。

## 主要一次情報（URL・日付）

- Dioxus v0.7.4 リリース（2026-03-28, FFI / mobile 設定）: https://github.com/DioxusLabs/dioxus/releases/tag/v0.7.4
- Dioxus releases（0.8.0-alpha.3, 2026-10-16）: https://github.com/DioxusLabs/dioxus/releases
- Dioxus mobile guide 0.7: https://dioxuslabs.com/learn/0.7/guides/platforms/mobile/
- manganis::ffi docs: https://docs.rs/dioxus/latest/dioxus/prelude/manganis/attr.ffi.html
- Blitz roadmap（beta.4 2026-10-16, IME composition は Backlog）: https://blitz.is/roadmap
- Blitz IME PR（open）: https://github.com/DioxusLabs/blitz/issues/1086
- Slint 1.19（2026-11-16）: https://slint.dev/blog/slint-1.19-released
- Slint 1.15（2026-02-04, safe area / virtual keyboard）: https://slint.dev/blog/slint-1.15-released
- Slint mobile 一般 / iOS docs: https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/general/ , https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/ios/
- Slint モバイル IME PR #10557（2026-10-20 クローズ）: https://github.com/slint-ui/slint/pull/10557
- Slint #14968（2026-11-15 open）: https://github.com/slint-ui/slint/issues/14968
- Slint ライセンス: https://github.com/slint-ui/slint/blob/master/LICENSE.md
- Robrix 1.0.0-alpha.1（Makepad 2.0, 初出 2026-05-05）: https://github.com/project-robius/robrix/releases/tag/v1.0.0-alpha.1
- Moly v0.2.4（モバイル IME 初期対応）: https://github.com/moly-ai/moly-ai/releases/tag/v0.2.4
- Makepad a11y issue #196: https://github.com/makepad/makepad/issues/196
- egui IME PR #7967（2026-03-24 マージ）: https://github.com/emilk/egui/pull/7967 / Android issue #8884: https://github.com/emilk/egui/issues/8884 / #7941: https://github.com/emilk/egui/issues/7941
- Iced README（対応プラットフォーム）: https://github.com/iced-rs/iced
- Freya platforms docs: https://docs.rs/freya/latest/freya/_docs/platforms/index.html
- AccessKit releases（2026-10-25）: https://github.com/AccessKit/accesskit/releases
- UniFFI CHANGELOG（0.32.x）: https://github.com/mozilla/uniffi-rs/blob/main/CHANGELOG.md
- UniFFI futures: https://mozilla.github.io/uniffi-rs/latest/futures.html / callback interfaces: https://mozilla.github.io/uniffi-rs/latest/types/callback_interfaces.html
- UniFFI streams issue #2755: https://github.com/mozilla/uniffi-rs/issues/2755
- Element X iOS: https://github.com/element-hq/element-x-ios
- Crux releases: https://github.com/redbadger/crux/releases
- relm4 CHANGES: https://github.com/Relm4/Relm4/blob/main/CHANGES.md
- Zed Linux IME issue #54959: https://github.com/zed-industries/zed/issues/54959
- Android WebView insets / IME: https://developer.android.com/develop/ui/views/layout/webapps/understand-window-insets

## 採用しなかった / 優先度を下げた情報源
- finds.dev、codeassuranceindex、NUIF research、upd.dev ミラー: 二次情報のため、本文では [二次] として扱った。
- Reddit スレッド: 実運用の雰囲気をつかむ参考にとどめ、根拠には使っていない。
