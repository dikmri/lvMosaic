use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use crate::model::VideoInfo;

// ---- バイナリ解決 -------------------------------------------------------

#[cfg(windows)]
const FFMPEG_BIN:  &str = "ffmpeg.exe";
#[cfg(windows)]
const FFPROBE_BIN: &str = "ffprobe.exe";
#[cfg(not(windows))]
const FFMPEG_BIN:  &str = "ffmpeg";
#[cfg(not(windows))]
const FFPROBE_BIN: &str = "ffprobe";

/// ffmpegのパスを返す。
/// 優先順: (1) 自身のexeの隣 (同梱バイナリ) → (2) PATH
fn ffmpeg_path() -> PathBuf {
    bundled_path(FFMPEG_BIN).unwrap_or_else(|| PathBuf::from(FFMPEG_BIN))
}

/// ffprobeのパスを返す。
fn ffprobe_path() -> PathBuf {
    bundled_path(FFPROBE_BIN).unwrap_or_else(|| PathBuf::from(FFPROBE_BIN))
}

/// exeの隣にバイナリがあればそのパスを返す
fn bundled_path(name: &str) -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let candidate = exe_dir.join(name);
    candidate.exists().then_some(candidate)
}

// ---- 公開API -----------------------------------------------------------

/// FFmpegが利用可能か確認し、パスと由来を返す
pub struct FfmpegStatus {
    pub available: bool,
    pub bundled: bool,
    pub path: PathBuf,
}

pub fn check_ffmpeg() -> FfmpegStatus {
    let path = ffmpeg_path();
    let bundled = bundled_path(FFMPEG_BIN).is_some();
    let available = Command::new(&path)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    FfmpegStatus { available, bundled, path }
}

/// FFprobeでメタ情報を取得する
pub fn probe_video(path: &Path) -> Result<VideoInfo> {
    let output = Command::new(ffprobe_path())
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_streams",
            "-show_format",
            path.to_str().context("invalid path")?,
        ])
        .output()
        .context("ffprobe の起動に失敗しました。")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("動画を読み込めませんでした。\nmp4ファイルであることを確認してください。\n\n{}", stderr);
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("ffprobe の出力を解析できませんでした。")?;

    parse_probe_output(&json)
}

fn parse_probe_output(json: &serde_json::Value) -> Result<VideoInfo> {
    let streams = json["streams"].as_array().context("streams が見つかりません")?;

    let video_stream = streams.iter().find(|s| s["codec_type"] == "video")
        .context("映像ストリームが見つかりません")?;

    let width  = video_stream["width"].as_u64().context("width が取得できません")? as u32;
    let height = video_stream["height"].as_u64().context("height が取得できません")? as u32;
    let fps    = parse_fps(video_stream["r_frame_rate"].as_str().unwrap_or("30/1"));
    let video_codec = video_stream["codec_name"].as_str().unwrap_or("unknown").to_string();

    let total_frames = if let Some(nf) = video_stream["nb_frames"].as_str() {
        nf.parse::<u64>().unwrap_or(0)
    } else {
        0
    };

    let duration_sec = if let Some(d) = video_stream["duration"].as_str() {
        d.parse::<f64>().unwrap_or(0.0)
    } else if let Some(d) = json["format"]["duration"].as_str() {
        d.parse::<f64>().unwrap_or(0.0)
    } else {
        0.0
    };

    let total_frames = if total_frames == 0 {
        (duration_sec * fps).round() as u64
    } else {
        total_frames
    };

    let audio_stream = streams.iter().find(|s| s["codec_type"] == "audio");
    let has_audio    = audio_stream.is_some();
    let audio_codec  = audio_stream.and_then(|s| s["codec_name"].as_str()).map(|s| s.to_string());
    let bitrate      = json["format"]["bit_rate"].as_str().and_then(|s| s.parse::<u64>().ok());

    Ok(VideoInfo { width, height, fps, total_frames, duration_sec, video_codec, has_audio, audio_codec, bitrate })
}

/// FFmpeg が見つからない場合のエラーメッセージ (プラットフォーム別)
pub fn ffmpeg_not_found_message() -> String {
    #[cfg(windows)]
    return "FFmpegが見つかりません。\n\
             ffmpeg.exe と ffprobe.exe をこのアプリと同じフォルダに置いてください。\n\n\
             入手先: https://github.com/BtbN/FFmpeg-Builds/releases\n\
             (ffmpeg-master-latest-win64-lgpl-shared.zip の bin/ 内にあります)".to_string();
    #[cfg(not(windows))]
    return "FFmpegが見つかりません。\n\
             ffmpeg と ffprobe をこのアプリと同じフォルダに置くか、\n\
             PATH の通った場所にインストールしてください。".to_string();
}

pub(crate) fn parse_fps(r_frame_rate: &str) -> f64 {
    let parts: Vec<&str> = r_frame_rate.split('/').collect();
    if parts.len() == 2 {
        let num = parts[0].parse::<f64>().unwrap_or(30.0);
        let den = parts[1].parse::<f64>().unwrap_or(1.0);
        if den != 0.0 { num / den } else { 30.0 }
    } else {
        r_frame_rate.parse::<f64>().unwrap_or(30.0)
    }
}

/// 指定フレームのRGB24データを取得する
pub fn decode_frame(video_path: &Path, frame_index: u64, fps: f64, width: u32, height: u32) -> Result<Vec<u8>> {
    let time_sec = frame_index as f64 / fps;

    let output = Command::new(ffmpeg_path())
        .args([
            "-ss", &format!("{:.6}", time_sec),
            "-i", video_path.to_str().context("invalid path")?,
            "-vframes", "1",
            "-f", "rawvideo",
            "-pix_fmt", "rgb24",
            "-s", &format!("{}x{}", width, height),
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .context("ffmpeg の起動に失敗しました。")?;

    let expected = (width * height * 3) as usize;
    if output.stdout.len() < expected {
        return Ok(vec![0u8; expected]);
    }

    Ok(output.stdout[..expected].to_vec())
}

/// 指定フレームを低解像度でデコードする
pub fn decode_frame_scaled(
    video_path: &Path,
    frame_index: u64,
    fps: f64,
    orig_width: u32,
    orig_height: u32,
    scale: f32,
) -> Result<(Vec<u8>, u32, u32)> {
    let w = (((orig_width  as f32 * scale) as u32).max(1) / 2) * 2;
    let h = (((orig_height as f32 * scale) as u32).max(1) / 2) * 2;
    let data = decode_frame(video_path, frame_index, fps, w, h)?;
    Ok((data, w, h))
}

/// 出力ファイルパスを生成する (_mosaic, 連番)
pub fn make_output_path(input_path: &Path) -> PathBuf {
    let stem = input_path.file_stem().unwrap_or_default().to_string_lossy();
    let dir  = input_path.parent().unwrap_or(Path::new("."));

    let base = dir.join(format!("{}_mosaic.mp4", stem));
    if !base.exists() { return base; }

    for i in 1..=999 {
        let candidate = dir.join(format!("{}_mosaic_{:03}.mp4", stem, i));
        if !candidate.exists() { return candidate; }
    }

    base
}

/// エクスポート用: ffmpegにRGB24フレームをパイプで渡してmp4を生成する
pub fn spawn_encoder(
    output_path: &Path,
    video_path: &Path,
    width: u32,
    height: u32,
    fps: f64,
    start_sec: f64,
    duration_sec: f64,
    crf: u32,
    preset: &str,
) -> Result<std::process::Child> {
    let child = Command::new(ffmpeg_path())
        .args([
            "-y",
            // 映像入力: stdin (rawvideo RGB24)
            "-f", "rawvideo",
            "-pix_fmt", "rgb24",
            "-s", &format!("{}x{}", width, height),
            "-r", &fps.to_string(),
            "-i", "pipe:0",
            // 音声入力: 元動画 (指定範囲のみ)
            "-ss", &format!("{:.6}", start_sec),
            "-t",  &format!("{:.6}", duration_sec),
            "-i", video_path.to_str().context("invalid path")?,
            // マッピング
            "-map", "0:v:0",
            "-map", "1:a?",
            // エンコード設定
            "-c:v", "libx264",
            "-preset", preset,
            "-crf", &crf.to_string(),
            "-pix_fmt", "yuv420p",
            "-c:a", "copy",
            "-movflags", "+faststart",
            output_path.to_str().context("invalid output path")?,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("ffmpeg (エンコーダ) の起動に失敗しました。")?;

    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    // T-01: FPS文字列パース
    #[test]
    fn test_parse_fps_integer_fraction() {
        assert!((parse_fps("30/1") - 30.0).abs() < 1e-6);
    }

    #[test]
    fn test_parse_fps_ntsc() {
        assert!((parse_fps("2997/100") - 29.97).abs() < 0.01);
    }

    #[test]
    fn test_parse_fps_film() {
        assert!((parse_fps("24000/1001") - 23.976).abs() < 0.01);
    }

    #[test]
    fn test_parse_fps_plain_number() {
        assert!((parse_fps("25") - 25.0).abs() < 1e-6);
    }

    #[test]
    fn test_parse_fps_zero_denominator_falls_back() {
        assert!((parse_fps("30/0") - 30.0).abs() < 1e-6);
    }

    #[test]
    fn test_parse_fps_invalid_falls_back() {
        assert!((parse_fps("bad") - 30.0).abs() < 1e-6);
    }

    // T-03: 出力パス生成
    #[test]
    fn test_make_output_path_suffix() {
        let path = std::path::PathBuf::from("/tmp/video.mp4");
        let out = make_output_path(&path);
        let s = out.to_string_lossy();
        assert!(s.contains("video_mosaic"), "output: {}", s);
        assert!(s.ends_with(".mp4"), "output: {}", s);
    }
}
