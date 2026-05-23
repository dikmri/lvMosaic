use egui::{Color32, Ui};
use crate::model::{ExportQuality, ExportSettings, MosaicRegion};

pub struct PropertiesResult {
    pub delete_mosaic: bool,
    pub add_keyframe: bool,
    pub delete_keyframe: bool,
    pub export_requested: bool,
    pub save_requested: bool,
    pub mosaic_changed: bool,
}

pub fn show_properties(
    ui: &mut Ui,
    selected: Option<&mut MosaicRegion>,
    export: &mut ExportSettings,
    current_frame: u64,
    total_frames: u64,
    exporting: bool,
    export_progress: f32,
) -> PropertiesResult {
    let mut result = PropertiesResult {
        delete_mosaic: false,
        add_keyframe: false,
        delete_keyframe: false,
        export_requested: false,
        save_requested: false,
        mosaic_changed: false,
    };

    ui.vertical(|ui| {
        // モザイク範囲プロパティ
        if let Some(region) = selected {
            ui.heading("モザイク範囲");
            ui.separator();

            ui.label(format!("ID: {}", region.id));

            ui.horizontal(|ui| {
                ui.label("名前:");
                ui.text_edit_singleline(&mut region.name);
            });

            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("開始フレーム:");
                let mut start = region.start_frame as i64;
                if ui.add(egui::DragValue::new(&mut start).range(0..=(region.end_frame as i64))).changed() {
                    region.start_frame = start.max(0) as u64;
                    result.mosaic_changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("終了フレーム:");
                let mut end = region.end_frame as i64;
                if ui.add(egui::DragValue::new(&mut end).range((region.start_frame as i64)..=(total_frames as i64))).changed() {
                    region.end_frame = end as u64;
                    result.mosaic_changed = true;
                }
            });

            ui.add_space(4.0);
            ui.label(format!("現在フレーム: {}", current_frame));

            // 現在フレームの補間値を表示・編集
            if let Some(mut vals) = region.interpolate(current_frame) {
                ui.separator();
                ui.label("位置・サイズ");

                ui.horizontal(|ui| {
                    ui.label("X:");
                    if ui.add(egui::DragValue::new(&mut vals.center_x).speed(1.0)).changed() {
                        region.ensure_keyframe_at(current_frame);
                        if let Some(kf) = region.keyframes.iter_mut().find(|k| k.frame == current_frame) {
                            kf.center_x = vals.center_x;
                        }
                        result.mosaic_changed = true;
                    }
                    ui.label("Y:");
                    if ui.add(egui::DragValue::new(&mut vals.center_y).speed(1.0)).changed() {
                        region.ensure_keyframe_at(current_frame);
                        if let Some(kf) = region.keyframes.iter_mut().find(|k| k.frame == current_frame) {
                            kf.center_y = vals.center_y;
                        }
                        result.mosaic_changed = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("W:");
                    if ui.add(egui::DragValue::new(&mut vals.width).speed(1.0).range(1.0..=9999.0)).changed() {
                        region.ensure_keyframe_at(current_frame);
                        if let Some(kf) = region.keyframes.iter_mut().find(|k| k.frame == current_frame) {
                            kf.width = vals.width;
                        }
                        result.mosaic_changed = true;
                    }
                    ui.label("H:");
                    if ui.add(egui::DragValue::new(&mut vals.height).speed(1.0).range(1.0..=9999.0)).changed() {
                        region.ensure_keyframe_at(current_frame);
                        if let Some(kf) = region.keyframes.iter_mut().find(|k| k.frame == current_frame) {
                            kf.height = vals.height;
                        }
                        result.mosaic_changed = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("回転:");
                    if ui.add(egui::DragValue::new(&mut vals.rotation_deg).speed(0.5).suffix("°")).changed() {
                        region.ensure_keyframe_at(current_frame);
                        if let Some(kf) = region.keyframes.iter_mut().find(|k| k.frame == current_frame) {
                            kf.rotation_deg = vals.rotation_deg;
                        }
                        result.mosaic_changed = true;
                    }
                });
            }

            ui.horizontal(|ui| {
                ui.label("モザイク粒度:");
                let mut size = region.mosaic_size as i32;
                if ui.add(egui::DragValue::new(&mut size).range(2..=64)).changed() {
                    region.mosaic_size = size as u32;
                    result.mosaic_changed = true;
                }
            });

            ui.add_space(6.0);
            ui.separator();

            // キーフレーム操作
            let has_kf = region.keyframes.iter().any(|k| k.frame == current_frame);
            ui.horizontal(|ui| {
                if ui.button("+ キーフレーム追加").clicked() {
                    result.add_keyframe = true;
                }
                if has_kf {
                    if ui.button("× キーフレーム削除").clicked() {
                        result.delete_keyframe = true;
                    }
                }
            });

            ui.add_space(4.0);
            if ui.add(egui::Button::new("モザイク範囲を削除").fill(Color32::from_rgb(160, 50, 50))).clicked() {
                result.delete_mosaic = true;
            }

            ui.add_space(8.0);
            ui.separator();
        } else {
            ui.label("モザイク範囲が選択されていません");
            ui.add_space(8.0);
            ui.separator();
        }

        // 出力設定
        ui.heading("出力設定");
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("開始側除去:");
            let mut start = export.trim_start_frames as i64;
            if ui.add(egui::DragValue::new(&mut start).range(0..=9999)).changed() {
                export.trim_start_frames = start as u64;
            }
            ui.label("フレーム");
        });
        ui.horizontal(|ui| {
            ui.label("終了側除去:");
            let mut end = export.trim_end_frames as i64;
            if ui.add(egui::DragValue::new(&mut end).range(0..=9999)).changed() {
                export.trim_end_frames = end as u64;
            }
            ui.label("フレーム");
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("品質:");
            for q in [ExportQuality::Fast, ExportQuality::Standard, ExportQuality::High] {
                let selected = export.quality == q;
                if ui.selectable_label(selected, q.label()).clicked() {
                    export.quality = q;
                }
            }
        });

        ui.add_space(6.0);

        if exporting {
            ui.add(egui::ProgressBar::new(export_progress).text(format!("{:.0}%", export_progress * 100.0)));
        } else {
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Export").fill(Color32::from_rgb(50, 120, 50))).clicked() {
                    result.export_requested = true;
                }
                if ui.button("Save").clicked() {
                    result.save_requested = true;
                }
            });
        }
    });

    result
}
