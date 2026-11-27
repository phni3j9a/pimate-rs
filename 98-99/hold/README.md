# #99：指で本文の位置を保持する（PR #100 レビュー再検証）

Pixel 3、ライト、文字倍率1.0。`dev.pimate.app.rs.debug`、試験専用 Host (`PIMATE_DEMO_DESKTOP_REVIEW=1 scripts/android-demo.py`) だけを使用。修正前 `c4e23d8` と修正後 `d702674` を同じ手順で録画した。`p2-tail.mp4` は同じ修正後 APK で別の模擬 Host のストリーミングを無操作で録画したもの。

- `adb shell screenrecord --size 720x1440 --bit-rate 16000000 --time-limit 18` で一覧から `/demo/android` を開く。
- 開いてから約4.8秒で `adb shell 'input swipe 500 900 500 1220 1800 & pid=$!; sleep 1.4; kill -STOP $pid; sleep 3; kill -CONT $pid; wait $pid'`。止めたのは **swipeの入力プロセスのみ**。ドラッグ途中で指を3秒静止させてから離す。アプリ・Host・録画は動かし続ける。
- 指を止めている **8.0–9.8秒**を全フレーム解析。`track-hold.py` は修正前動画の「変更内容」(x=363, y=712, 116×24px) を両動画の同じ列で縦に探索する。修正前は712→469px（始点・終点の差243px）、修正後は**連続110フレームすべて863px**。動画間の絶対座標ではなく、各動画内で指を止めた後に同じ文字が動くかを比較した。差し替わる候補を除外したり、座標を補間したりしていない。
- 無操作の場合：親ディレクトリの `track.py` と同じ x=69 の24×8テンプレートで照合。`p2-tail.csv` のアイコン出現 **0.917778秒から17.969489秒まで連続1,024フレームすべて1100px**。
