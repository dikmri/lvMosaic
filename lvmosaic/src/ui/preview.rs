use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
use crate::model::{InterpValues, MosaicRegion};
use crate::mosaic::{rotated_rect_corners_in_screen, screen_to_video};

const HANDLE_RADIUS: f32 = 6.0;
const HANDLE_COLOR: Color32 = Color32::from_rgb(255, 200, 50);
const SELECTED_STROKE: Color32 = Color32::from_rgb(255, 200, 50);
const UNSELECTED_STROKE: Color32 = Color32::from_rgba_premultiplied(200, 200, 200, 180);
const ROTATE_HANDLE_OFFSET: f32 = 28.0;

/// マウス操作の状態
#[derive(Debug, Clone, PartialEq)]
pub enum DragState {
    None,
    Creating { start_video: (f32, f32) },
    Moving { mosaic_id: String, start_mouse: Pos2, start_center: (f32, f32) },
    Resizing { mosaic_id: String, handle: ResizeHandle, start_mouse: Pos2, orig_vals: InterpValues },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResizeHandle {
    TopLeft, TopRight, BottomLeft, BottomRight,
    Top, Bottom, Left, Right,
}

pub struct PreviewResult {
    pub drag_state: DragState,
    pub selected_id: Option<String>,
    pub new_mosaic_rect: Option<(f32, f32, f32, f32)>, // center_x, center_y, width, height
    pub mosaic_updates: Vec<(String, InterpValues)>,
}

pub fn show_preview(
    ui: &mut Ui,
    texture: Option<&egui::TextureHandle>,
    video_width: u32,
    video_height: u32,
    mosaics: &[MosaicRegion],
    current_frame: u64,
    selected_id: Option<&str>,
    drag_state: &DragState,
) -> PreviewResult {
    let available = ui.available_size();
    let aspect = video_width as f32 / video_height as f32;
    let display_w = available.x.min(available.y * aspect);
    let display_h = display_w / aspect;

    let (response, painter) = ui.allocate_painter(
        Vec2::new(display_w, display_h),
        Sense::click_and_drag(),
    );

    let video_rect = response.rect;

    // 背景
    painter.rect_filled(video_rect, 0.0, Color32::BLACK);

    // ビデオフレーム描画
    if let Some(tex) = texture {
        painter.image(tex.id(), video_rect, Rect::from_min_max(
            egui::pos2(0.0, 0.0),
            egui::pos2(1.0, 1.0),
        ), Color32::WHITE);
    }

    // ドラッグ中の新規作成矩形プレビュー
    if let DragState::Creating { start_video } = drag_state {
        if let Some(mouse) = response.hover_pos() {
            let (mx, my) = screen_to_video(mouse, video_rect, video_width, video_height);
            let (sx, sy) = *start_video;
            let cx = (sx + mx) / 2.0;
            let cy = (sy + my) / 2.0;
            let w = (mx - sx).abs();
            let h = (my - sy).abs();
            let fake_vals = InterpValues { center_x: cx, center_y: cy, width: w, height: h, rotation_deg: 0.0 };
            draw_mosaic_rect(&painter, &fake_vals, video_rect, video_width, video_height, true);
        }
    }

    // 各モザイク範囲を描画
    for region in mosaics {
        if !region.is_active_at(current_frame) {
            continue;
        }
        if let Some(vals) = region.interpolate(current_frame) {
            let is_selected = selected_id == Some(&region.id);
            draw_mosaic_rect(&painter, &vals, video_rect, video_width, video_height, is_selected);

            if is_selected {
                draw_resize_handles(&painter, &vals, video_rect, video_width, video_height);
            }
        }
    }

    // インタラクション結果を構築
    build_result(
        response,
        video_rect,
        video_width,
        video_height,
        mosaics,
        current_frame,
        selected_id,
        drag_state,
    )
}

fn draw_mosaic_rect(
    painter: &egui::Painter,
    vals: &InterpValues,
    video_rect: Rect,
    video_width: u32,
    video_height: u32,
    selected: bool,
) {
    let corners = rotated_rect_corners_in_screen(vals, video_rect, video_width, video_height);
    let stroke_color = if selected { SELECTED_STROKE } else { UNSELECTED_STROKE };
    let stroke = Stroke::new(2.0, stroke_color);

    painter.line_segment([corners[0], corners[1]], stroke);
    painter.line_segment([corners[1], corners[2]], stroke);
    painter.line_segment([corners[2], corners[3]], stroke);
    painter.line_segment([corners[3], corners[0]], stroke);

    // 半透明の塗り
    let fill = if selected {
        Color32::from_rgba_premultiplied(255, 200, 50, 20)
    } else {
        Color32::from_rgba_premultiplied(200, 200, 255, 15)
    };
    painter.add(egui::Shape::convex_polygon(corners.to_vec(), fill, Stroke::NONE));
}

fn draw_resize_handles(
    painter: &egui::Painter,
    vals: &InterpValues,
    video_rect: Rect,
    video_width: u32,
    video_height: u32,
) {
    let corners = rotated_rect_corners_in_screen(vals, video_rect, video_width, video_height);

    // 4隅ハンドル
    for c in &corners {
        painter.circle_filled(*c, HANDLE_RADIUS, HANDLE_COLOR);
        painter.circle_stroke(*c, HANDLE_RADIUS, Stroke::new(1.5, Color32::BLACK));
    }

    // 辺の中点ハンドル
    let mid_handles = [
        midpoint(corners[0], corners[3]), // top
        midpoint(corners[1], corners[2]), // bottom
        midpoint(corners[0], corners[1]), // left
        midpoint(corners[3], corners[2]), // right
    ];
    for m in &mid_handles {
        painter.circle_filled(*m, HANDLE_RADIUS * 0.8, HANDLE_COLOR);
        painter.circle_stroke(*m, HANDLE_RADIUS * 0.8, Stroke::new(1.0, Color32::BLACK));
    }
}

fn midpoint(a: Pos2, b: Pos2) -> Pos2 {
    egui::pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)
}

fn build_result(
    response: Response,
    video_rect: Rect,
    video_width: u32,
    video_height: u32,
    mosaics: &[MosaicRegion],
    current_frame: u64,
    selected_id: Option<&str>,
    drag_state: &DragState,
) -> PreviewResult {
    let mut result = PreviewResult {
        drag_state: drag_state.clone(),
        selected_id: selected_id.map(|s| s.to_string()),
        new_mosaic_rect: None,
        mosaic_updates: Vec::new(),
    };

    // クリックで選択
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let hit = hit_test_mosaics(pos, mosaics, current_frame, video_rect, video_width, video_height);
            result.selected_id = hit;
            result.drag_state = DragState::None;
        }
    }

    // ドラッグ開始
    if response.drag_started() {
        if let Some(pos) = response.interact_pointer_pos() {
            let hit = hit_test_mosaics(pos, mosaics, current_frame, video_rect, video_width, video_height);
            if let Some(id) = hit {
                // 既存モザイクを移動
                if let Some(region) = mosaics.iter().find(|m| m.id == id) {
                    if let Some(vals) = region.interpolate(current_frame) {
                        result.drag_state = DragState::Moving {
                            mosaic_id: id.clone(),
                            start_mouse: pos,
                            start_center: (vals.center_x, vals.center_y),
                        };
                        result.selected_id = Some(id);
                    }
                }
            } else {
                // 新規モザイク作成
                let (vx, vy) = screen_to_video(pos, video_rect, video_width, video_height);
                result.drag_state = DragState::Creating { start_video: (vx, vy) };
            }
        }
    }

    // ドラッグ中
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            match &drag_state.clone() {
                DragState::Moving { mosaic_id, start_mouse, start_center } => {
                    let dx = (pos.x - start_mouse.x) / video_rect.width() * video_width as f32;
                    let dy = (pos.y - start_mouse.y) / video_rect.height() * video_height as f32;
                    if let Some(region) = mosaics.iter().find(|m| &m.id == mosaic_id) {
                        if let Some(mut vals) = region.interpolate(current_frame) {
                            vals.center_x = start_center.0 + dx;
                            vals.center_y = start_center.1 + dy;
                            result.mosaic_updates.push((mosaic_id.clone(), vals));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // ドラッグ終了
    if response.drag_stopped() {
        match &drag_state.clone() {
            DragState::Creating { start_video } => {
                if let Some(pos) = response.interact_pointer_pos() {
                    let (mx, my) = screen_to_video(pos, video_rect, video_width, video_height);
                    let (sx, sy) = *start_video;
                    let w = (mx - sx).abs();
                    let h = (my - sy).abs();
                    if w > 5.0 && h > 5.0 {
                        let cx = (sx + mx) / 2.0;
                        let cy = (sy + my) / 2.0;
                        result.new_mosaic_rect = Some((cx, cy, w, h));
                    }
                }
                result.drag_state = DragState::None;
            }
            DragState::Moving { .. } => {
                result.drag_state = DragState::None;
            }
            _ => {
                result.drag_state = DragState::None;
            }
        }
    }

    result
}

fn hit_test_mosaics(
    pos: Pos2,
    mosaics: &[MosaicRegion],
    current_frame: u64,
    video_rect: Rect,
    video_width: u32,
    video_height: u32,
) -> Option<String> {
    // 逆順で判定 (後から追加したものが優先)
    for region in mosaics.iter().rev() {
        if !region.is_active_at(current_frame) {
            continue;
        }
        if let Some(vals) = region.interpolate(current_frame) {
            let (vx, vy) = screen_to_video(pos, video_rect, video_width, video_height);
            if is_inside_rotated_rect_video(vx, vy, &vals) {
                return Some(region.id.clone());
            }
        }
    }
    None
}

fn is_inside_rotated_rect_video(vx: f32, vy: f32, vals: &InterpValues) -> bool {
    let theta = vals.rotation_deg.to_radians();
    let cos_t = theta.cos();
    let sin_t = theta.sin();
    let dx = vx - vals.center_x;
    let dy = vy - vals.center_y;
    let local_x = dx * cos_t + dy * sin_t;
    let local_y = -dx * sin_t + dy * cos_t;
    local_x.abs() <= vals.width / 2.0 && local_y.abs() <= vals.height / 2.0
}
