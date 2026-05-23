use egui::{Color32, Rect, Sense, Stroke, Ui, Vec2};
use crate::model::MosaicRegion;

const TIMELINE_HEIGHT: f32 = 20.0;
const TRACK_HEIGHT: f32 = 18.0;
const KEYFRAME_RADIUS: f32 = 5.0;
const TRACK_BG: Color32 = Color32::from_rgb(45, 45, 45);
const TRACK_BAR: Color32 = Color32::from_rgb(80, 130, 180);
const TRACK_BAR_SELECTED: Color32 = Color32::from_rgb(120, 170, 220);
const KEYFRAME_COLOR: Color32 = Color32::from_rgb(255, 200, 50);
const PLAYHEAD_COLOR: Color32 = Color32::from_rgb(255, 80, 80);

pub struct TimelineResult {
    pub seek_to: Option<u64>,
    pub selected_id: Option<String>,
}

pub fn show_timeline(
    ui: &mut Ui,
    mosaics: &[MosaicRegion],
    current_frame: u64,
    total_frames: u64,
    selected_id: Option<&str>,
) -> TimelineResult {
    let mut result = TimelineResult { seek_to: None, selected_id: selected_id.map(|s| s.to_string()) };

    if total_frames == 0 {
        return result;
    }

    let available_width = ui.available_width();

    // シークバー (全体)
    let (seekbar_resp, seekbar_painter) = ui.allocate_painter(
        Vec2::new(available_width, TIMELINE_HEIGHT),
        Sense::click_and_drag(),
    );
    let seekbar_rect = seekbar_resp.rect;

    seekbar_painter.rect_filled(seekbar_rect, 2.0, Color32::from_rgb(30, 30, 30));

    let playhead_x = seekbar_rect.left() + (current_frame as f32 / total_frames as f32) * seekbar_rect.width();
    seekbar_painter.line_segment(
        [egui::pos2(playhead_x, seekbar_rect.top()), egui::pos2(playhead_x, seekbar_rect.bottom())],
        Stroke::new(2.0, PLAYHEAD_COLOR),
    );

    // クリック/ドラッグでシーク
    if seekbar_resp.clicked() || seekbar_resp.dragged() {
        if let Some(pos) = seekbar_resp.interact_pointer_pos() {
            let t = ((pos.x - seekbar_rect.left()) / seekbar_rect.width()).clamp(0.0, 1.0);
            result.seek_to = Some((t * total_frames as f32) as u64);
        }
    }

    // モザイクトラック
    for region in mosaics {
        let is_selected = selected_id == Some(&region.id);

        let (track_resp, track_painter) = ui.allocate_painter(
            Vec2::new(available_width, TRACK_HEIGHT + 4.0),
            Sense::click(),
        );
        let track_rect = track_resp.rect;

        track_painter.rect_filled(track_rect, 0.0, TRACK_BG);

        let bar_color = if is_selected { TRACK_BAR_SELECTED } else { TRACK_BAR };

        let start_x = region.start_frame as f32 / total_frames as f32 * track_rect.width();
        let end_x = (region.end_frame as f32 + 1.0) / total_frames as f32 * track_rect.width();
        let bar_rect = Rect::from_min_max(
            egui::pos2(track_rect.left() + start_x, track_rect.top() + 2.0),
            egui::pos2(track_rect.left() + end_x, track_rect.bottom() - 2.0),
        );
        track_painter.rect_filled(bar_rect, 3.0, bar_color);

        // キーフレームのドット
        for kf in &region.keyframes {
            if kf.frame >= region.start_frame && kf.frame <= region.end_frame {
                let kf_x = track_rect.left() + kf.frame as f32 / total_frames as f32 * track_rect.width();
                let kf_y = (track_rect.top() + track_rect.bottom()) / 2.0;
                track_painter.circle_filled(egui::pos2(kf_x, kf_y), KEYFRAME_RADIUS, KEYFRAME_COLOR);
                track_painter.circle_stroke(
                    egui::pos2(kf_x, kf_y),
                    KEYFRAME_RADIUS,
                    Stroke::new(1.0, Color32::BLACK),
                );
            }
        }

        // プレイヘッド
        let ph_x = track_rect.left() + current_frame as f32 / total_frames as f32 * track_rect.width();
        track_painter.line_segment(
            [egui::pos2(ph_x, track_rect.top()), egui::pos2(ph_x, track_rect.bottom())],
            Stroke::new(1.5, PLAYHEAD_COLOR),
        );

        // 名前ラベル
        track_painter.text(
            egui::pos2(bar_rect.left() + 4.0, bar_rect.center().y),
            egui::Align2::LEFT_CENTER,
            &region.name,
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );

        // クリックで選択、キーフレームへシーク
        if track_resp.clicked() {
            if let Some(pos) = track_resp.interact_pointer_pos() {
                let t = ((pos.x - track_rect.left()) / track_rect.width()).clamp(0.0, 1.0);
                let clicked_frame = (t * total_frames as f32) as u64;

                // 近くのキーフレームがあればそこへシーク
                let nearest_kf = region.keyframes.iter()
                    .min_by_key(|kf| (kf.frame as i64 - clicked_frame as i64).unsigned_abs());

                if let Some(kf) = nearest_kf {
                    let kf_x = track_rect.left() + kf.frame as f32 / total_frames as f32 * track_rect.width();
                    if (pos.x - kf_x).abs() < 10.0 {
                        result.seek_to = Some(kf.frame);
                    }
                }
                result.selected_id = Some(region.id.clone());
            }
        }
    }

    result
}
