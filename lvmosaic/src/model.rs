use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub app_name: String,
    pub video_path: PathBuf,
    pub video: VideoInfo,
    pub export: ExportSettings,
    pub mosaics: Vec<MosaicRegion>,
}

impl Project {
    pub fn new(video_path: PathBuf, video: VideoInfo) -> Self {
        Self {
            version: 1,
            app_name: "lvMosaic".to_string(),
            video_path,
            video,
            export: ExportSettings::default(),
            mosaics: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub total_frames: u64,
    pub duration_sec: f64,
    pub video_codec: String,
    pub has_audio: bool,
    pub audio_codec: Option<String>,
    pub bitrate: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSettings {
    pub trim_start_frames: u64,
    pub trim_end_frames: u64,
    pub quality: ExportQuality,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            trim_start_frames: 0,
            trim_end_frames: 0,
            quality: ExportQuality::Standard,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExportQuality {
    Fast,
    Standard,
    High,
}

impl ExportQuality {
    pub fn label(&self) -> &'static str {
        match self {
            ExportQuality::Fast => "高速",
            ExportQuality::Standard => "標準",
            ExportQuality::High => "高品質",
        }
    }

    pub fn crf(&self) -> u32 {
        match self {
            ExportQuality::Fast => 24,
            ExportQuality::Standard => 22,
            ExportQuality::High => 18,
        }
    }

    pub fn preset(&self) -> &'static str {
        match self {
            ExportQuality::Fast => "veryfast",
            ExportQuality::Standard => "medium",
            ExportQuality::High => "slow",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MosaicRegion {
    pub id: String,
    pub name: String,
    pub start_frame: u64,
    pub end_frame: u64,
    pub mosaic_size: u32,
    pub keyframes: Vec<MosaicKeyframe>,
}

impl MosaicRegion {
    pub fn new(id: String, name: String, start_frame: u64, end_frame: u64, initial_kf: MosaicKeyframe) -> Self {
        Self {
            id,
            name,
            start_frame,
            end_frame,
            mosaic_size: 16,
            keyframes: vec![initial_kf],
        }
    }

    /// 指定フレームで有効かどうか
    pub fn is_active_at(&self, frame: u64) -> bool {
        frame >= self.start_frame && frame <= self.end_frame
    }

    /// 指定フレームでの補間済みキーフレーム値を返す
    pub fn interpolate(&self, frame: u64) -> Option<InterpValues> {
        if self.keyframes.is_empty() {
            return None;
        }

        let kfs = &self.keyframes;

        // フレームより前のキーフレームがなければ先頭キーフレームを使用
        if frame <= kfs[0].frame {
            return Some(InterpValues::from_keyframe(&kfs[0]));
        }

        // フレームより後のキーフレームがなければ末尾キーフレームを使用
        let last = &kfs[kfs.len() - 1];
        if frame >= last.frame {
            return Some(InterpValues::from_keyframe(last));
        }

        // 前後のキーフレームを探して線形補間
        for i in 0..kfs.len() - 1 {
            let prev = &kfs[i];
            let next = &kfs[i + 1];
            if frame >= prev.frame && frame <= next.frame {
                let t = (frame - prev.frame) as f32 / (next.frame - prev.frame) as f32;
                return Some(InterpValues {
                    center_x: lerp(prev.center_x, next.center_x, t),
                    center_y: lerp(prev.center_y, next.center_y, t),
                    width: lerp(prev.width, next.width, t),
                    height: lerp(prev.height, next.height, t),
                    rotation_deg: lerp(prev.rotation_deg, next.rotation_deg, t),
                });
            }
        }

        None
    }

    /// 現在フレームにキーフレームを追加または上書きする
    pub fn set_keyframe(&mut self, kf: MosaicKeyframe) {
        let pos = self.keyframes.iter().position(|k| k.frame == kf.frame);
        match pos {
            Some(i) => self.keyframes[i] = kf,
            None => {
                self.keyframes.push(kf);
                self.keyframes.sort_by_key(|k| k.frame);
            }
        }
    }

    /// 現在フレームにキーフレームがなければ補間値でキーフレームを作成する
    pub fn ensure_keyframe_at(&mut self, frame: u64) {
        if self.keyframes.iter().any(|k| k.frame == frame) {
            return;
        }
        if let Some(v) = self.interpolate(frame) {
            self.set_keyframe(MosaicKeyframe {
                frame,
                center_x: v.center_x,
                center_y: v.center_y,
                width: v.width,
                height: v.height,
                rotation_deg: v.rotation_deg,
            });
        }
    }

    /// 現在フレームのキーフレームを削除する
    pub fn remove_keyframe_at(&mut self, frame: u64) {
        self.keyframes.retain(|k| k.frame != frame);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MosaicKeyframe {
    pub frame: u64,
    pub center_x: f32,
    pub center_y: f32,
    pub width: f32,
    pub height: f32,
    pub rotation_deg: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterpValues {
    pub center_x: f32,
    pub center_y: f32,
    pub width: f32,
    pub height: f32,
    pub rotation_deg: f32,
}

impl InterpValues {
    pub fn from_keyframe(kf: &MosaicKeyframe) -> Self {
        Self {
            center_x: kf.center_x,
            center_y: kf.center_y,
            width: kf.width,
            height: kf.height,
            rotation_deg: kf.rotation_deg,
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
