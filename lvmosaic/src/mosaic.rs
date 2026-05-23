use crate::model::{InterpValues, MosaicRegion};

/// フレーム画像にモザイクを適用する
/// pixels: RGB24形式のバイト列 (width * height * 3)
pub fn apply_mosaics(pixels: &mut [u8], width: u32, height: u32, regions: &[&MosaicRegion], frame: u64) {
    for region in regions {
        if !region.is_active_at(frame) {
            continue;
        }
        if let Some(vals) = region.interpolate(frame) {
            apply_mosaic_region(pixels, width, height, &vals, region.mosaic_size);
        }
    }
}

fn apply_mosaic_region(pixels: &mut [u8], width: u32, height: u32, vals: &InterpValues, mosaic_size: u32) {
    let block = mosaic_size.max(1) as i32;
    let theta = vals.rotation_deg.to_radians();
    let cos_t = theta.cos();
    let sin_t = theta.sin();

    // 外接矩形を計算して走査範囲を絞る
    let (bbox_min_x, bbox_min_y, bbox_max_x, bbox_max_y) =
        bounding_box(vals.center_x, vals.center_y, vals.width, vals.height, theta);

    let x0 = (bbox_min_x as i32).max(0);
    let y0 = (bbox_min_y as i32).max(0);
    let x1 = (bbox_max_x as i32 + 1).min(width as i32);
    let y1 = (bbox_max_y as i32 + 1).min(height as i32);

    if x0 >= x1 || y0 >= y1 {
        return;
    }

    // ブロック単位でモザイク処理
    let bx0 = (x0 / block) * block;
    let by0 = (y0 / block) * block;

    let mut bx = bx0;
    while bx < x1 {
        let mut by = by0;
        while by < y1 {
            // ブロック内で矩形と交差するか確認
            if block_intersects_rotated_rect(bx, by, block, vals, cos_t, sin_t) {
                // ブロック代表色: 左上ピクセルの色
                let rep_x = bx.min(width as i32 - 1).max(0) as u32;
                let rep_y = by.min(height as i32 - 1).max(0) as u32;
                let idx = (rep_y * width + rep_x) as usize * 3;
                let (r, g, b) = if idx + 2 < pixels.len() {
                    (pixels[idx], pixels[idx + 1], pixels[idx + 2])
                } else {
                    (0, 0, 0)
                };

                // ブロック内の矩形内ピクセルに代表色を塗る
                for dy in 0..block {
                    let py = by + dy;
                    if py < 0 || py >= height as i32 {
                        continue;
                    }
                    for dx in 0..block {
                        let px = bx + dx;
                        if px < 0 || px >= width as i32 {
                            continue;
                        }
                        if is_inside_rotated_rect(px as f32, py as f32, vals, cos_t, sin_t) {
                            let i = (py as u32 * width + px as u32) as usize * 3;
                            if i + 2 < pixels.len() {
                                pixels[i] = r;
                                pixels[i + 1] = g;
                                pixels[i + 2] = b;
                            }
                        }
                    }
                }
            }
            by += block;
        }
        bx += block;
    }
}

fn is_inside_rotated_rect(px: f32, py: f32, vals: &InterpValues, cos_t: f32, sin_t: f32) -> bool {
    let dx = px - vals.center_x;
    let dy = py - vals.center_y;
    // 逆回転
    let local_x = dx * cos_t + dy * sin_t;
    let local_y = -dx * sin_t + dy * cos_t;
    local_x.abs() <= vals.width / 2.0 && local_y.abs() <= vals.height / 2.0
}

fn block_intersects_rotated_rect(bx: i32, by: i32, block: i32, vals: &InterpValues, cos_t: f32, sin_t: f32) -> bool {
    // ブロックの4隅のいずれかが矩形内にあればtrue (簡易判定)
    for (dx, dy) in [(0, 0), (block - 1, 0), (0, block - 1), (block - 1, block - 1)] {
        if is_inside_rotated_rect((bx + dx) as f32, (by + dy) as f32, vals, cos_t, sin_t) {
            return true;
        }
    }
    // ブロック中心でも判定
    let cx = bx as f32 + block as f32 / 2.0;
    let cy = by as f32 + block as f32 / 2.0;
    is_inside_rotated_rect(cx, cy, vals, cos_t, sin_t)
}

/// 回転矩形の外接矩形を返す (min_x, min_y, max_x, max_y)
fn bounding_box(cx: f32, cy: f32, w: f32, h: f32, theta: f32) -> (f32, f32, f32, f32) {
    let hw = w / 2.0;
    let hh = h / 2.0;
    let cos_t = theta.cos();
    let sin_t = theta.sin();

    let corners = [
        (cx + hw * cos_t - hh * sin_t, cy + hw * sin_t + hh * cos_t),
        (cx - hw * cos_t - hh * sin_t, cy - hw * sin_t + hh * cos_t),
        (cx + hw * cos_t + hh * sin_t, cy + hw * sin_t - hh * cos_t),
        (cx - hw * cos_t + hh * sin_t, cy - hw * sin_t - hh * cos_t),
    ];

    let min_x = corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min);
    let min_y = corners.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);
    let max_x = corners.iter().map(|c| c.0).fold(f32::NEG_INFINITY, f32::max);
    let max_y = corners.iter().map(|c| c.1).fold(f32::NEG_INFINITY, f32::max);

    (min_x, min_y, max_x, max_y)
}

/// プレビュー用: egui座標系での回転矩形の4隅を返す
/// video_rect: ビデオが描画されているegui画面上の矩形
pub fn rotated_rect_corners_in_screen(
    vals: &InterpValues,
    video_rect: egui::Rect,
    video_width: u32,
    video_height: u32,
) -> [egui::Pos2; 4] {
    let scale_x = video_rect.width() / video_width as f32;
    let scale_y = video_rect.height() / video_height as f32;

    let cx = video_rect.left() + vals.center_x * scale_x;
    let cy = video_rect.top() + vals.center_y * scale_y;
    let hw = vals.width * scale_x / 2.0;
    let hh = vals.height * scale_y / 2.0;
    let theta = vals.rotation_deg.to_radians();
    let cos_t = theta.cos();
    let sin_t = theta.sin();

    [
        egui::pos2(cx + hw * cos_t - hh * sin_t, cy + hw * sin_t + hh * cos_t),
        egui::pos2(cx - hw * cos_t - hh * sin_t, cy - hw * sin_t + hh * cos_t),
        egui::pos2(cx - hw * cos_t + hh * sin_t, cy - hw * sin_t - hh * cos_t),
        egui::pos2(cx + hw * cos_t + hh * sin_t, cy + hw * sin_t - hh * cos_t),
    ]
}

/// スクリーン座標からビデオ座標へ変換
pub fn screen_to_video(screen_pos: egui::Pos2, video_rect: egui::Rect, video_width: u32, video_height: u32) -> (f32, f32) {
    let x = (screen_pos.x - video_rect.left()) / video_rect.width() * video_width as f32;
    let y = (screen_pos.y - video_rect.top()) / video_rect.height() * video_height as f32;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{InterpValues, MosaicKeyframe, MosaicRegion};

    fn vals_no_rot(cx: f32, cy: f32, w: f32, h: f32) -> InterpValues {
        InterpValues { center_x: cx, center_y: cy, width: w, height: h, rotation_deg: 0.0 }
    }

    // T-12: 非アクティブフレームでは画素変更なし
    #[test]
    fn test_apply_mosaics_inactive_frame() {
        let w = 20u32; let h = 20u32;
        let mut pixels = vec![255u8, 0, 0].repeat((w * h) as usize);
        let original = pixels.clone();
        let region = MosaicRegion::new("m1".to_string(), "M1".to_string(), 10, 100,
            MosaicKeyframe { frame: 10, center_x: 10.0, center_y: 10.0,
                             width: 8.0, height: 8.0, rotation_deg: 0.0 });
        apply_mosaics(&mut pixels, w, h, &[&region], 5); // frame 5, not active
        assert_eq!(pixels, original);
    }

    // T-13: アクティブフレームでモザイク適用
    #[test]
    fn test_apply_mosaics_active_frame_no_panic() {
        let w = 40u32; let h = 40u32;
        let mut pixels: Vec<u8> = (0..(w * h * 3)).map(|i| (i % 256) as u8).collect();
        let region = MosaicRegion::new("m1".to_string(), "M1".to_string(), 0, 100,
            MosaicKeyframe { frame: 0, center_x: 20.0, center_y: 20.0,
                             width: 16.0, height: 16.0, rotation_deg: 0.0 });
        apply_mosaics(&mut pixels, w, h, &[&region], 0);
        assert_eq!(pixels.len(), (w * h * 3) as usize);
    }

    // T-14: bounding_box 回転なし
    #[test]
    fn test_bounding_box_no_rotation() {
        let (min_x, min_y, max_x, max_y) = bounding_box(50.0, 50.0, 20.0, 10.0, 0.0);
        assert!((min_x - 40.0).abs() < 1e-3, "min_x={}", min_x);
        assert!((max_x - 60.0).abs() < 1e-3, "max_x={}", max_x);
        assert!((min_y - 45.0).abs() < 1e-3, "min_y={}", min_y);
        assert!((max_y - 55.0).abs() < 1e-3, "max_y={}", max_y);
    }

    #[test]
    fn test_bounding_box_90_degrees() {
        // 90度回転: width と height が入れ替わった形の外接矩形
        let theta = std::f32::consts::FRAC_PI_2;
        let (min_x, min_y, max_x, max_y) = bounding_box(0.0, 0.0, 20.0, 10.0, theta);
        // 幅20,高さ10 を90度回転 → 幅10,高さ20 相当の外接矩形
        assert!((max_x - min_x - 10.0).abs() < 0.1, "width={}", max_x - min_x);
        assert!((max_y - min_y - 20.0).abs() < 0.1, "height={}", max_y - min_y);
    }

    // T-15: is_inside_rotated_rect
    #[test]
    fn test_inside_rotated_rect_center() {
        let v = vals_no_rot(50.0, 50.0, 20.0, 10.0);
        assert!(is_inside_rotated_rect(50.0, 50.0, &v, 1.0, 0.0));
    }

    #[test]
    fn test_inside_rotated_rect_outside_x() {
        let v = vals_no_rot(50.0, 50.0, 20.0, 10.0);
        assert!(!is_inside_rotated_rect(62.0, 50.0, &v, 1.0, 0.0));
    }

    #[test]
    fn test_inside_rotated_rect_outside_y() {
        let v = vals_no_rot(50.0, 50.0, 20.0, 10.0);
        assert!(!is_inside_rotated_rect(50.0, 56.0, &v, 1.0, 0.0));
    }

    #[test]
    fn test_inside_rotated_rect_just_inside() {
        let v = vals_no_rot(50.0, 50.0, 20.0, 10.0);
        assert!(is_inside_rotated_rect(59.9, 54.9, &v, 1.0, 0.0));
    }
}
