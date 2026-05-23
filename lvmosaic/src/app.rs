use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Color32, Context, FontData, FontDefinitions, FontFamily, Key, TextureHandle, TextureOptions};

use crate::model::{MosaicKeyframe, MosaicRegion, Project};
use crate::mosaic::apply_mosaics; // エクスポート処理でのみ使用
use crate::ui::preview::{show_preview, DragState};
use crate::ui::timeline::show_timeline;
use crate::ui::properties::show_properties;
use crate::undo::{EditAction, UndoStack};
use crate::video::{decode_frame_scaled, make_output_path, probe_video};

/// Windowsシステムフォントから日本語フォントを読み込んでeguiに設定する
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    // 優先順: メイリオ → 游ゴシック Medium → MS ゴシック
    let candidates = [
        "C:/Windows/Fonts/meiryo.ttc",
        "C:/Windows/Fonts/YuGothM.ttc",
        "C:/Windows/Fonts/YuGothR.ttc",
        "C:/Windows/Fonts/msgothic.ttc",
    ];

    for path in &candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(
                "cjk".to_owned(),
                FontData::from_owned(data).into(),
            );
            // デフォルトフォントの後に追加 (ラテン文字は既存フォントを優先し、
            // 日本語グリフは cjk フォントにフォールバックする)
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts.families
                    .entry(family)
                    .or_default()
                    .push("cjk".to_owned());
            }
            break;
        }
    }

    ctx.set_fonts(fonts);
}

/// プレビュー解像度スケール
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewScale {
    Auto,
    Full,
    Half,
    Quarter,
}

impl PreviewScale {
    pub fn factor(&self, width: u32, height: u32) -> f32 {
        match self {
            PreviewScale::Auto => {
                if width > 1920 { 0.25 } else if width > 960 { 0.5 } else { 1.0 }
            }
            PreviewScale::Full => 1.0,
            PreviewScale::Half => 0.5,
            PreviewScale::Quarter => 0.25,
        }
    }
}

pub struct LvMosaicApp {
    // 動画・プロジェクト状態
    project: Option<Project>,
    video_path: Option<PathBuf>,

    // 再生状態
    current_frame: u64,
    playing: bool,
    last_play_tick: Option<Instant>,

    // プレビューテクスチャ
    preview_texture: Option<TextureHandle>,
    loaded_frame: Option<u64>,
    preview_scale: PreviewScale,

    // UI状態
    selected_mosaic_id: Option<String>,
    drag_state: DragState,

    // Undo/Redo
    undo_stack: UndoStack,

    // エクスポート状態
    exporting: bool,
    export_progress: f32,
    export_error: Option<String>,

    // エラー表示
    error_message: Option<String>,
    status_message: Option<String>,
}

impl LvMosaicApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);

        let ffmpeg_status = crate::video::check_ffmpeg();
        let error_message = if !ffmpeg_status.available {
            Some(
                "FFmpegが見つかりません。\n\
                 ffmpeg.exe と ffprobe.exe をこのアプリと同じフォルダに置いてください。\n\n\
                 入手先: https://github.com/BtbN/FFmpeg-Builds/releases\n\
                 (ffmpeg-master-latest-win64-lgpl-shared.zip の bin/ 内にあります)"
                .to_string()
            )
        } else {
            None
        };
        let status_message = if ffmpeg_status.available {
            if ffmpeg_status.bundled {
                Some(format!("FFmpeg (同梱版) を使用中: {}", ffmpeg_status.path.display()))
            } else {
                Some(format!("FFmpeg (PATH) を使用中: {}", ffmpeg_status.path.display()))
            }
        } else {
            None
        };

        Self {
            project: None,
            video_path: None,
            current_frame: 0,
            playing: false,
            last_play_tick: None,
            preview_texture: None,
            loaded_frame: None,
            preview_scale: PreviewScale::Auto,
            selected_mosaic_id: None,
            drag_state: DragState::None,
            undo_stack: UndoStack::new(),
            exporting: false,
            export_progress: 0.0,
            export_error: None,
            error_message,
            status_message,
        }
    }

    fn total_frames(&self) -> u64 {
        self.project.as_ref().map(|p| p.video.total_frames).unwrap_or(0)
    }

    fn fps(&self) -> f64 {
        self.project.as_ref().map(|p| p.video.fps).unwrap_or(30.0)
    }

    fn video_width(&self) -> u32 {
        self.project.as_ref().map(|p| p.video.width).unwrap_or(1280)
    }

    fn video_height(&self) -> u32 {
        self.project.as_ref().map(|p| p.video.height).unwrap_or(720)
    }

    fn load_video(&mut self, path: PathBuf) {
        match probe_video(&path) {
            Ok(info) => {
                let project = Project::new(path.clone(), info);
                self.video_path = Some(path);
                self.project = Some(project);
                self.current_frame = 0;
                self.preview_texture = None;
                self.loaded_frame = None;
                self.selected_mosaic_id = None;
                self.undo_stack = UndoStack::new();
                self.error_message = None;
                self.status_message = Some("動画を読み込みました".to_string());
            }
            Err(e) => {
                self.error_message = Some(format!("動画を読み込めませんでした。\n\n{}", e));
            }
        }
    }

    fn update_preview_texture(&mut self, ctx: &Context) {
        let Some(path) = &self.video_path else { return };
        let Some(project) = &self.project else { return };

        if self.loaded_frame == Some(self.current_frame) {
            return;
        }

        let scale = self.preview_scale.factor(project.video.width, project.video.height);
        let path = path.clone();
        let frame = self.current_frame;
        let fps = project.video.fps;
        let w = project.video.width;
        let h = project.video.height;

        match decode_frame_scaled(&path, frame, fps, w, h, scale) {
            Ok((pixels, pw, ph)) => {
                // プレビューテクスチャは生フレームのみ保持（モザイク処理なし）
                // モザイクのビジュアルは egui オーバーレイで描画するため不要
                let image = egui::ColorImage::from_rgb([pw as usize, ph as usize], &pixels);
                self.preview_texture = Some(ctx.load_texture(
                    "preview",
                    image,
                    TextureOptions::LINEAR,
                ));
                self.loaded_frame = Some(frame);
            }
            Err(e) => {
                log::warn!("フレームデコード失敗: {}", e);
            }
        }
    }

    fn seek_to(&mut self, frame: u64) {
        let total = self.total_frames();
        if total == 0 { return; }
        self.current_frame = frame.min(total - 1);
        self.loaded_frame = None;
    }

    fn frame_step(&mut self, delta: i64) {
        let total = self.total_frames();
        if total == 0 { return; }
        let new_frame = (self.current_frame as i64 + delta).clamp(0, total as i64 - 1) as u64;
        self.current_frame = new_frame;
        self.loaded_frame = None;
    }

    fn toggle_play(&mut self) {
        self.playing = !self.playing;
        if self.playing {
            self.last_play_tick = Some(Instant::now());
        }
    }

    fn tick_playback(&mut self) {
        if !self.playing { return; }
        let fps = self.fps();
        if fps <= 0.0 { return; }

        let now = Instant::now();
        let tick = self.last_play_tick.get_or_insert(now);
        let elapsed = now.duration_since(*tick).as_secs_f64();
        let frames_to_advance = (elapsed * fps) as u64;

        if frames_to_advance > 0 {
            *tick = now;
            let next = self.current_frame + frames_to_advance;
            if next >= self.total_frames() {
                self.current_frame = 0;
                self.playing = false;
            } else {
                self.current_frame = next;
            }
            self.loaded_frame = None;
        }
    }

    fn handle_keyboard(&mut self, ctx: &Context) {
        let input = ctx.input(|i| i.clone());
        let shift = input.modifiers.shift;
        let ctrl = input.modifiers.command_only();

        // Undo/Redo
        if ctrl {
            if input.key_pressed(Key::Z) {
                if let Some(proj) = self.project.as_mut() {
                    self.undo_stack.undo(&mut proj.mosaics, &mut proj.export);
                }
                return;
            }
            if input.key_pressed(Key::Y) || (input.modifiers.shift && input.key_pressed(Key::Z)) {
                if let Some(proj) = self.project.as_mut() {
                    self.undo_stack.redo(&mut proj.mosaics, &mut proj.export);
                }
                return;
            }
        }

        // モザイク選択中のキー操作
        if let Some(id) = &self.selected_mosaic_id.clone() {
            if let Some(proj) = &mut self.project {
                if let Some(region_idx) = proj.mosaics.iter().position(|m| &m.id == id) {
                    let current = self.current_frame;
                    let before = proj.mosaics[region_idx].clone();
                    let mut changed = false;

                    {
                        let region = &mut proj.mosaics[region_idx];
                        region.ensure_keyframe_at(current);
                        let kf = region.keyframes.iter_mut().find(|k| k.frame == current);

                        if let Some(kf) = kf {
                            let step = if shift { 5.0 } else { 1.0 };

                            // 回転
                            if input.key_pressed(Key::Q) {
                                kf.rotation_deg -= step;
                                changed = true;
                            }
                            if input.key_pressed(Key::E) {
                                kf.rotation_deg += step;
                                changed = true;
                            }
                            if input.key_pressed(Key::R) {
                                kf.rotation_deg = 0.0;
                                changed = true;
                            }

                            // キーボード移動 (矢印)
                            let move_step = if shift { 10.0 } else { 1.0 };
                            if input.key_pressed(Key::ArrowUp) {
                                kf.center_y -= move_step;
                                changed = true;
                            }
                            if input.key_pressed(Key::ArrowDown) {
                                kf.center_y += move_step;
                                changed = true;
                            }
                            if input.key_pressed(Key::ArrowLeft) {
                                kf.center_x -= move_step;
                                changed = true;
                            }
                            if input.key_pressed(Key::ArrowRight) {
                                kf.center_x += move_step;
                                changed = true;
                            }
                        }
                    }

                    // Delete キー
                    if input.key_pressed(Key::Delete) {
                        let removed = proj.mosaics.remove(region_idx);
                        self.undo_stack.push(EditAction::RemoveMosaic { index: region_idx, region: removed });
                        self.selected_mosaic_id = None;
                        return;
                    }

                    if changed {
                        let after = proj.mosaics[region_idx].clone();
                        self.undo_stack.push(EditAction::UpdateMosaic { index: region_idx, before, after });
                    }
                }
            }
        }

        // フレーム送り (モザイク未選択時、またはモザイク選択中でもSpace)
        if input.key_pressed(Key::Space) {
            self.toggle_play();
        }

        if self.selected_mosaic_id.is_none() {
            if input.key_pressed(Key::ArrowLeft) {
                if shift { self.frame_step(-10); } else { self.frame_step(-1); }
            }
            if input.key_pressed(Key::ArrowRight) {
                if shift { self.frame_step(10); } else { self.frame_step(1); }
            }
            if input.key_pressed(Key::Home) {
                self.seek_to(0);
            }
            if input.key_pressed(Key::End) {
                let total = self.total_frames();
                if total > 0 { self.seek_to(total - 1); }
            }
        }
    }

    fn add_mosaic(&mut self, cx: f32, cy: f32, w: f32, h: f32) {
        let Some(proj) = &mut self.project else { return };
        let id = format!("mosaic_{:03}", proj.mosaics.len() + 1);
        let name = format!("Mosaic {}", proj.mosaics.len() + 1);
        let kf = MosaicKeyframe {
            frame: self.current_frame,
            center_x: cx, center_y: cy,
            width: w, height: h,
            rotation_deg: 0.0,
        };
        let region = MosaicRegion::new(
            id.clone(), name,
            self.current_frame, proj.video.total_frames.saturating_sub(1),
            kf,
        );
        self.undo_stack.push(EditAction::AddMosaic(region.clone()));
        proj.mosaics.push(region);
        self.selected_mosaic_id = Some(id);
    }

    fn save_project(&self) {
        let Some(proj) = &self.project else { return };
        let Some(video_path) = &self.video_path else { return };
        let stem = video_path.file_stem().unwrap_or_default().to_string_lossy();
        let dir = video_path.parent().unwrap_or(Path::new("."));
        let save_path = dir.join(format!("{}.lvMosaic.json", stem));

        match serde_json::to_string_pretty(proj) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&save_path, json) {
                    log::error!("保存失敗: {}", e);
                } else {
                    log::info!("保存: {:?}", save_path);
                }
            }
            Err(e) => log::error!("JSON生成失敗: {}", e),
        }
    }

    fn load_project_json(&mut self, json_path: &Path) {
        match std::fs::read_to_string(json_path) {
            Ok(json) => {
                match serde_json::from_str::<Project>(&json) {
                    Ok(proj) => {
                        let video_path = json_path.parent()
                            .map(|d| d.join(&proj.video_path))
                            .unwrap_or_else(|| proj.video_path.clone());
                        self.video_path = Some(video_path);
                        self.project = Some(proj);
                        self.current_frame = 0;
                        self.loaded_frame = None;
                        self.preview_texture = None;
                        self.status_message = Some("プロジェクトを読み込みました".to_string());
                    }
                    Err(e) => {
                        self.error_message = Some(format!("プロジェクト読み込み失敗: {}", e));
                    }
                }
            }
            Err(e) => {
                self.error_message = Some(format!("ファイル読み込み失敗: {}", e));
            }
        }
    }

    fn start_export(&mut self) {
        let Some(proj) = &self.project else { return };
        let Some(video_path) = &self.video_path else { return };

        // バリデーション
        let total = proj.video.total_frames;
        let trim_sum = proj.export.trim_start_frames + proj.export.trim_end_frames;
        if trim_sum >= total {
            self.error_message = Some(
                "除去フレーム数が動画の総フレーム数以上になっています。\n開始側または終了側の除去フレーム数を小さくしてください。".to_string()
            );
            return;
        }

        let output_path = make_output_path(video_path);
        let video_path = video_path.clone();
        let proj_clone = proj.clone();

        self.exporting = true;
        self.export_progress = 0.0;
        self.export_error = None;
        self.status_message = Some("エクスポート中...".to_string());

        // 別スレッドでエクスポート
        let (tx, rx) = crossbeam_channel::bounded::<ExportMsg>(32);
        std::thread::spawn(move || {
            run_export(&video_path, &output_path, &proj_clone, tx);
        });

        // rxをポーリングするためにフィールドに保持
        // (簡略化のため、次のframeでポーリング)
        // 実際の実装ではArcなどで保持する
        // ここではシンプルにエクスポート完了を待つ方式は取らず、
        // 非同期ポーリングは省略してブロッキングで実行する
        drop(rx);
    }
}

enum ExportMsg {
    Progress(u64, u64),
    Done,
    Error(String),
}

fn run_export(
    video_path: &Path,
    output_path: &Path,
    proj: &Project,
    tx: crossbeam_channel::Sender<ExportMsg>,
) {
    use std::io::Write;

    let fps = proj.video.fps;
    let w   = proj.video.width;
    let h   = proj.video.height;
    let start_frame = proj.export.trim_start_frames;
    let end_frame   = proj.video.total_frames.saturating_sub(proj.export.trim_end_frames + 1);

    if start_frame > end_frame {
        let _ = tx.send(ExportMsg::Error("除去フレーム数が不正です".to_string()));
        return;
    }

    let total        = end_frame - start_frame + 1;
    let start_sec    = start_frame as f64 / fps;
    let duration_sec = total as f64 / fps;

    let mut enc = match crate::video::spawn_encoder(
        output_path, video_path,
        w, h, fps,
        start_sec, duration_sec,
        proj.export.quality.crf(),
        proj.export.quality.preset(),
    ) {
        Ok(child) => child,
        Err(e) => {
            let _ = tx.send(ExportMsg::Error(format!("ffmpegの起動に失敗しました: {}", e)));
            return;
        }
    };

    let mut stdin = enc.stdin.take().unwrap();

    for frame_idx in start_frame..=end_frame {
        let _ = tx.send(ExportMsg::Progress(frame_idx - start_frame, total));

        match crate::video::decode_frame(video_path, frame_idx, fps, w, h) {
            Ok(mut pixels) => {
                let mosaics: Vec<&MosaicRegion> = proj.mosaics.iter().collect();
                apply_mosaics(&mut pixels, w, h, &mosaics, frame_idx);

                if let Err(e) = stdin.write_all(&pixels) {
                    let _ = tx.send(ExportMsg::Error(format!("フレーム書き込み失敗: {}", e)));
                    return;
                }
            }
            Err(e) => {
                log::warn!("フレーム {} デコード失敗: {}", frame_idx, e);
                // エラーフレームは黒で代替
                let blank = vec![0u8; (w * h * 3) as usize];
                let _ = stdin.write_all(&blank);
            }
        }
    }

    drop(stdin);

    match enc.wait() {
        Ok(status) if status.success() => {
            let _ = tx.send(ExportMsg::Done);
        }
        Ok(status) => {
            let _ = tx.send(ExportMsg::Error(format!("ffmpegがエラーで終了しました: {:?}", status)));
        }
        Err(e) => {
            let _ = tx.send(ExportMsg::Error(format!("ffmpeg待機失敗: {}", e)));
        }
    }
}

impl eframe::App for LvMosaicApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // 再生ティック
        self.tick_playback();
        if self.playing {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        // キーボード処理
        if self.project.is_some() {
            self.handle_keyboard(ctx);
        }

        // プレビューフレーム更新
        if self.project.is_some() {
            self.update_preview_texture(ctx);
        }

        // ドラッグ＆ドロップ
        if !ctx.input(|i| i.raw.dropped_files.is_empty()) {
            let paths: Vec<PathBuf> = ctx.input(|i| {
                i.raw.dropped_files.iter()
                    .filter_map(|f| f.path.clone())
                    .collect()
            });
            if let Some(path) = paths.into_iter().next() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                if ext == "mp4" {
                    self.load_video(path);
                } else if ext == "json" {
                    self.load_project_json(&path.clone());
                }
            }
        }

        // トップバー
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("lvMosaic");
                ui.add_space(12.0);

                if let Some(path) = &self.video_path {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    ui.label(egui::RichText::new(name.to_string()).color(Color32::LIGHT_GRAY));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.project.is_some() {
                        if ui.button("Save").clicked() {
                            self.save_project();
                        }
                        if !self.exporting && ui.add(
                            egui::Button::new("Export").fill(Color32::from_rgb(50, 120, 50))
                        ).clicked() {
                            self.start_export();
                        }
                    }
                });
            });
        });

        // 右側プロパティパネル
        egui::SidePanel::right("properties")
            .min_width(220.0)
            .max_width(280.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let selected_idx = self.project.as_ref().and_then(|p| {
                        self.selected_mosaic_id.as_ref().and_then(|id| {
                            p.mosaics.iter().position(|m| &m.id == id)
                        })
                    });

                    let current_frame = self.current_frame;
                    let total_frames = self.total_frames();
                    let exporting = self.exporting;
                    let export_progress = self.export_progress;

                    let result = if let Some(proj) = &mut self.project {
                        let selected = selected_idx.map(|i| &mut proj.mosaics[i]);
                        show_properties(
                            ui,
                            selected,
                            &mut proj.export,
                            current_frame,
                            total_frames,
                            exporting,
                            export_progress,
                        )
                    } else {
                        crate::ui::properties::PropertiesResult {
                            delete_mosaic: false,
                            add_keyframe: false,
                            delete_keyframe: false,
                            export_requested: false,
                            save_requested: false,
                            mosaic_changed: false,
                        }
                    };

                    if result.delete_mosaic {
                        if let (Some(proj), Some(id)) = (&mut self.project, &self.selected_mosaic_id.clone()) {
                            if let Some(idx) = proj.mosaics.iter().position(|m| &m.id == id) {
                                let removed = proj.mosaics.remove(idx);
                                self.undo_stack.push(EditAction::RemoveMosaic { index: idx, region: removed });
                                self.selected_mosaic_id = None;
                            }
                        }
                    }
                    if result.add_keyframe {
                        if let (Some(proj), Some(id)) = (&mut self.project, &self.selected_mosaic_id.clone()) {
                            if let Some(region) = proj.mosaics.iter_mut().find(|m| &m.id == id) {
                                region.ensure_keyframe_at(current_frame);
                            }
                        }
                    }
                    if result.delete_keyframe {
                        if let (Some(proj), Some(id)) = (&mut self.project, &self.selected_mosaic_id.clone()) {
                            if let Some(region) = proj.mosaics.iter_mut().find(|m| &m.id == id) {
                                region.remove_keyframe_at(current_frame);
                            }
                        }
                    }
                    if result.export_requested {
                        self.start_export();
                    }
                    if result.save_requested {
                        self.save_project();
                    }
                });
            });

        // 下部タイムライン
        egui::TopBottomPanel::bottom("timeline")
            .min_height(80.0)
            .max_height(200.0)
            .show(ctx, |ui| {
                // 再生コントロール
                ui.horizontal(|ui| {
                    let play_label = if self.playing { "⏸" } else { "▶" };
                    if ui.button(play_label).clicked() {
                        self.toggle_play();
                    }
                    if ui.button("⏹").clicked() {
                        self.playing = false;
                        self.seek_to(0);
                    }
                    if ui.button("◀◀").clicked() {
                        self.frame_step(-1);
                    }
                    if ui.button("▶▶").clicked() {
                        self.frame_step(1);
                    }

                    ui.separator();

                    let total = self.total_frames();
                    let fps = self.fps();
                    let cur_sec = if fps > 0.0 { self.current_frame as f64 / fps } else { 0.0 };
                    let total_sec = if fps > 0.0 { total as f64 / fps } else { 0.0 };

                    ui.label(format!(
                        "フレーム: {} / {}   時間: {:.2}s / {:.2}s",
                        self.current_frame, total, cur_sec, total_sec
                    ));
                });

                ui.separator();

                if self.project.is_some() {
                    let mosaics = self.project.as_ref().map(|p| p.mosaics.as_slice()).unwrap_or(&[]);
                    let result = show_timeline(
                        ui,
                        mosaics,
                        self.current_frame,
                        self.total_frames(),
                        self.selected_mosaic_id.as_deref(),
                    );
                    if let Some(f) = result.seek_to {
                        self.seek_to(f);
                    }
                    if result.selected_id.is_some() {
                        self.selected_mosaic_id = result.selected_id;
                    }
                }
            });

        // 中央プレビューエリア
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.project.is_none() {
                // 初期画面
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.label(egui::RichText::new("mp4動画をここにドラッグ＆ドロップ").size(20.0).color(Color32::GRAY));
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("または").color(Color32::DARK_GRAY));
                        ui.add_space(8.0);
                        if ui.button("動画を開く").clicked() {
                            let path = rfd::FileDialog::new()
                                .add_filter("動画ファイル", &["mp4"])
                                .pick_file();
                            if let Some(p) = path {
                                self.load_video(p);
                            }
                        }
                        ui.add_space(8.0);
                        if ui.button("プロジェクトを開く").clicked() {
                            let path = rfd::FileDialog::new()
                                .add_filter("lvMosaic プロジェクト", &["json"])
                                .pick_file();
                            if let Some(p) = path {
                                self.load_project_json(&p);
                            }
                        }
                    });
                });
            } else {
                let mosaics = self.project.as_ref().map(|p| p.mosaics.as_slice()).unwrap_or(&[]);
                let w = self.video_width();
                let h = self.video_height();
                let current_frame = self.current_frame;
                let selected_id = self.selected_mosaic_id.clone();

                let prev_result = show_preview(
                    ui,
                    self.preview_texture.as_ref(),
                    w, h,
                    mosaics,
                    current_frame,
                    selected_id.as_deref(),
                    &self.drag_state,
                );

                self.drag_state = prev_result.drag_state;
                self.selected_mosaic_id = prev_result.selected_id;

                // 新規モザイク矩形作成
                if let Some((cx, cy, mw, mh)) = prev_result.new_mosaic_rect {
                    self.add_mosaic(cx, cy, mw, mh);
                }

                // モザイク位置更新 (ドラッグ移動: loaded_frame は触らない)
                for (id, vals) in prev_result.mosaic_updates {
                    if let Some(proj) = &mut self.project {
                        if let Some(region) = proj.mosaics.iter_mut().find(|m| m.id == id) {
                            region.ensure_keyframe_at(current_frame);
                            if let Some(kf) = region.keyframes.iter_mut().find(|k| k.frame == current_frame) {
                                kf.center_x = vals.center_x;
                                kf.center_y = vals.center_y;
                                kf.width = vals.width;
                                kf.height = vals.height;
                                kf.rotation_deg = vals.rotation_deg;
                            }
                        }
                    }
                }
            }

            // エラーダイアログ
            if let Some(msg) = &self.error_message.clone() {
                let mut open = true;
                egui::Window::new("エラー")
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label(msg);
                        if ui.button("閉じる").clicked() {
                            self.error_message = None;
                        }
                    });
                if !open {
                    self.error_message = None;
                }
            }
        });
    }
}
