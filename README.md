# lvMosaic (れべるもざいく)

動画モザイク編集に特化した、超軽量・超高速ネイティブアプリです。

A lightweight, fast, native desktop tool focused exclusively on adding mosaic regions to video files.

---

## 特徴 / Features

- **mp4動画をD&Dで即読み込み** — ドラッグ&ドロップで起動、即プレビュー
- **手動モザイク指定** — プレビュー上でドラッグするだけで矩形モザイクを作成
- **回転対応** — Q/Eキーでモザイク矩形を自由に回転
- **キーフレーム補間** — フレームごとに位置・サイズ・回転を調整、キーフレーム間は線形補間
- **先頭/末尾フレーム除去** — 不要なフレームを除去して出力
- **Undo/Redo** — Ctrl+Z / Ctrl+Y で全編集操作を取り消し・やり直し
- **プロジェクト保存** — `.lvMosaic.json` で編集状態を保存・再開
- **FFmpeg同梱** — FFmpegを同梱することでインストール不要で即使用可能

---

## スクリーンショット / Screenshot

```
+------------------------------------------------------------+
| lvMosaic                                   [Export] [Save] |
+------------------------------------------------------------+
|                                                            |
|                      Video Preview                         |
|             [ draggable mosaic rectangle ]                 |
|                                                            |
+------------------------------------+-----------------------+
| [▶] [◀◀] [▶▶]  frame: 42 / 300   | Properties            |
| mosaic_001  ●---●---●              | X / Y / W / H         |
| mosaic_002    ●-----●              | Rotation / Size        |
|                                    | Trim / Export          |
+------------------------------------+-----------------------+
```

---

## ダウンロード / Download

[Releases](../../releases) ページから最新版の `lvMosaic-windows-x64.zip` をダウンロードしてください。

解凍して `lvmosaic.exe` をダブルクリックするだけで起動します。FFmpegは同梱されています。

---

## 操作方法 / Usage

### 動画読み込み

- mp4ファイルをウィンドウにドラッグ&ドロップ
- または「動画を開く」ボタンからファイル選択

### モザイク作成

1. プレビュー上でドラッグしてモザイク矩形を作成
2. 矩形をドラッグして移動
3. Q/E キーで回転、R で回転リセット
4. 矢印キーで1px移動、Shift+矢印で10px移動

### フレーム操作

| キー | 動作 |
|---|---|
| Space | 再生 / 一時停止 |
| ← / → | 1フレーム移動 |
| Shift + ← / → | 10フレーム移動 |
| Home / End | 先頭 / 末尾へ |

### 出力

- 右パネルで先頭/末尾の除去フレーム数と品質を設定
- `Export` ボタンで `<元ファイル名>_mosaic.mp4` を出力

---

## ビルド方法 / Build

```sh
# 必要なもの: Rust (stable), FFmpeg (PATH)
cd lvmosaic
cargo build --release
```

FFmpegの同梱については [BUNDLING_FFMPEG.md](lvmosaic/BUNDLING_FFMPEG.md) を参照してください。

---

## ライセンス / License

MIT

### FFmpegについて

このアプリはリリースビルドに [FFmpeg](https://ffmpeg.org/) (LGPL v2.1) を同梱します。
FFmpegのソースコードは https://ffmpeg.org/download.html から入手できます。
