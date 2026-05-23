# FFmpeg 同梱方法

## 配布フォルダ構成

```
lvMosaic/
  lvmosaic.exe       ← アプリ本体
  ffmpeg.exe         ← FFmpeg バイナリ (同梱)
  ffprobe.exe        ← FFprobe バイナリ (同梱)
  LICENSE.ffmpeg.txt ← FFmpeg のライセンス表記 (必須)
```

## FFmpeg バイナリの入手

**LGPL版 (推奨)** — アプリ自体のソース公開不要

1. https://github.com/BtbN/FFmpeg-Builds/releases を開く
2. `ffmpeg-master-latest-win64-lgpl-shared.zip` をダウンロード
3. 解凍して `bin/` フォルダ内の以下2ファイルをアプリと同じフォルダへコピー:
   - `ffmpeg.exe`
   - `ffprobe.exe`

> ※ `-lgpl-shared` は動的リンク版のため DLL も含まれますが、
>   `-lgpl` (静的リンク) 版があればそちらの方がファイルが少なく済みます。

## ライセンス表記

同梱時は必ず `LICENSE.ffmpeg.txt` 等のファイルでFFmpegのライセンス（LGPL v2.1）を
アプリと一緒に配布してください。

FFmpeg のライセンス全文: https://ffmpeg.org/legal.html

---

## 動作確認フロー

アプリ起動時に自身のexeと同じフォルダを優先して探します:

```
1. <アプリのexeのフォルダ>/ffmpeg.exe  ← 同梱版を最優先
2. PATH 上の ffmpeg                     ← フォールバック
```

ffmpeg が見つからない場合は起動直後にエラーダイアログが表示されます。
