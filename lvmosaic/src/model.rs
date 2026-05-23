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

#[cfg(test)]
mod tests {
    use super::*;

    fn region(kfs: &[(u64, f32, f32)]) -> MosaicRegion {
        let first = kfs[0];
        let mut r = MosaicRegion::new(
            "t".to_string(), "T".to_string(), 0, 1000,
            MosaicKeyframe { frame: first.0, center_x: first.1, center_y: first.2,
                             width: 10.0, height: 10.0, rotation_deg: 0.0 },
        );
        for &(f, cx, cy) in kfs.iter().skip(1) {
            r.set_keyframe(MosaicKeyframe { frame: f, center_x: cx, center_y: cy,
                                            width: 10.0, height: 10.0, rotation_deg: 0.0 });
        }
        r
    }

    // T-04: キーフレーム線形補間
    #[test]
    fn test_interpolate_linear_midpoint() {
        let r = region(&[(0, 0.0, 0.0), (100, 100.0, 200.0)]);
        let v = r.interpolate(50).unwrap();
        assert!((v.center_x - 50.0).abs() < 1e-4, "cx={}", v.center_x);
        assert!((v.center_y - 100.0).abs() < 1e-4, "cy={}", v.center_y);
    }

    // T-05: キーフレームクランプ
    #[test]
    fn test_interpolate_clamp_before_first() {
        let r = region(&[(10, 42.0, 0.0)]);
        let v = r.interpolate(0).unwrap();
        assert!((v.center_x - 42.0).abs() < 1e-4);
    }

    #[test]
    fn test_interpolate_clamp_after_last() {
        let r = region(&[(10, 0.0, 0.0), (20, 100.0, 0.0)]);
        let v = r.interpolate(50).unwrap();
        assert!((v.center_x - 100.0).abs() < 1e-4);
    }

    // T-06: is_active_at
    #[test]
    fn test_is_active_at_boundaries() {
        let r = MosaicRegion::new("id".to_string(), "n".to_string(), 10, 50,
            MosaicKeyframe { frame: 10, center_x: 0.0, center_y: 0.0,
                             width: 10.0, height: 10.0, rotation_deg: 0.0 });
        assert!(!r.is_active_at(9));
        assert!(r.is_active_at(10));
        assert!(r.is_active_at(50));
        assert!(!r.is_active_at(51));
    }

    // T-07: ensure_keyframe_at で補間値キーフレーム作成
    #[test]
    fn test_ensure_keyframe_creates_interpolated() {
        let mut r = region(&[(0, 0.0, 0.0), (100, 100.0, 0.0)]);
        r.ensure_keyframe_at(50);
        let kf = r.keyframes.iter().find(|k| k.frame == 50).expect("kf not created");
        assert!((kf.center_x - 50.0).abs() < 1e-4);
    }

    // T-08: ensure_keyframe_at は既存を重複しない
    #[test]
    fn test_ensure_keyframe_no_duplicate() {
        let mut r = region(&[(50, 99.0, 0.0)]);
        r.ensure_keyframe_at(50);
        assert_eq!(r.keyframes.len(), 1);
        assert!((r.keyframes[0].center_x - 99.0).abs() < 1e-4);
    }

    #[test]
    fn test_lerp_midpoint() {
        assert!((lerp(0.0, 10.0, 0.5) - 5.0).abs() < 1e-6);
        assert!((lerp(0.0, 10.0, 0.0) - 0.0).abs() < 1e-6);
        assert!((lerp(0.0, 10.0, 1.0) - 10.0).abs() < 1e-6);
    }
}
