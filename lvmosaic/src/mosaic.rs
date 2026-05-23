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
