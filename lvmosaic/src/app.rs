use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Color32, Context, FontData, FontDefinitions, FontFamily, Key, TextureHandle, TextureOptions};
use crossbeam_channel::{Receiver, Sender, TryRecvError};

use crate::model::{MosaicKeyframe, MosaicRegion, Project};
use crate::mosaic::apply_mosaics;
use crate::ui::preview::{show_preview, DragState};
use crate::ui::timeline::show_timeline;
use crate::ui::properties::show_properties;
use crate::undo::{EditAction, UndoStack};
use crate::video::{decode_frame_scaled, make_output_path, probe_video};

// ---- フォント設定 ----------------------------------------------------------

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
            fonts.font_data.insert("cjk".to_owned(), FontData::from_owned(data).into());
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("cjk".to_owned());
            }
            break;
        }
    }

    ctx.set_fonts(fonts);
}

// ---- バックグラウンドデコーダ -----------------------------------------------

struct DecodeRequest {
    path: PathBuf,
    frame: u64,
    fps: f64,
    width: u32,
    height: u32,
    scale: f32,
}

struct DecodeResult {
    frame: u64,
    pixels: Vec<u8>,
    pw: u32,
    ph: u32,
}

/// UIスレッドとは独立して動くデコードワーカー。
/// チャンネルからリクエストを受け取り、古いリクエストを読み捨てて
/// 最新フレームのみをデコードし結果を返す。
fn decode_worker(
    req_rx: Receiver<Option<DecodeRequest>>,
    res_tx: Sender<DecodeResult>,
) {
    loop {
        let mut req = match req_rx.recv() {
            Ok(Some(r)) => r,
            _ => break, // None or disconnected → shutdown
        };
        // 古いリクエストを全部読み捨て、最新だけ残す
        while let Ok(Some(newer)) = req_rx.try_recv() {
            req = newer;
        }
        match decode_frame_scaled(&req.path, req.frame, req.fps, req.width, req.height, req.scale) {
            Ok((pixels, pw, ph)) => {
                let _ = res_tx.send(DecodeResult { frame: req.frame, pixels, pw, ph });
            }
            Err(e) => log::warn!("フレームデコード失敗: {}", e),
        }
    }
}

// ---- エクスポートメッセージ ---------------------------------------------------

enum ExportMsg {
    Progress(u64, u64),
    Done,
    Error(String),
}

// ---- プレビュースケール -------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum PreviewScale {
    Auto,
    Full,
    Half,
    Quarter,
}

impl PreviewScale {
    pub fn factor(&self, width: u32, _height: u32) -> f32 {
        match self {
            PreviewScale::Auto => {
                if width > 1920 { 0.25 } else if width > 960 { 0.5 } else { 1.0 }
            }
            PreviewScale::Full    => 1.0,
            PreviewScale::Half    => 0.5,
            PreviewScale::Quarter => 0.25,
        }
    }
}

// ---- メインアプリ -----------------------------------------------------------

pub struct LvMosaicApp {
    // 動画・プロジェクト状態
    project:    Option<Project>,
    video_path: Option<PathBuf>,

    // 再生状態
    current_frame:  u64,
    playing:        bool,
    _last_play_tick: Option<Instant>,

    // プレビューテクスチャ
    preview_texture: Option<TextureHandle>,
    loaded_frame:    Option<u64>,    // 現在テクスチャに表示中のフレーム番号
    preview_scale:   PreviewScale,

    // バックグラウンドデコーダ (UIスレッドをブロックしない)
    decode_tx:            Sender<Option<DecodeRequest>>,
    decode_rx:            Receiver<DecodeResult>,
    pending_decode_frame: Option<u64>, // 現在リクエスト中のフレーム番号

    // UI状態
    selected_mosaic_id: Option<String>,
    drag_state:         DragState,

    // Undo/Redo
    undo_stack: UndoStack,

    // モザイクID生成カウンタ (単調増加で衝突防止)
    mosaic_counter: u32,

    // エクスポート状態
    exporting:        bool,
    export_progress:  f32,
    export_rx:        Option<Receiver<ExportMsg>>,

    // メッセージ表示
    error_message:  Option<String>,
    status_message: Option<String>,
}

impl LvMosaicApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);

        // バックグラウンドデコードワーカーを起動
        let (req_tx, req_rx) = crossbeam_channel::unbounded::<Option<DecodeRequest>>();
        let (res_tx, res_rx) = crossbeam_channel::bounded::<DecodeResult>(2);
        std::thread::spawn(|| decode_worker(req_rx, res_tx));

        let ffmpeg_status = crate::video::check_ffmpeg();
        let error_message = if !ffmpeg_status.available {
            Some(crate::video::ffmpeg_not_found_message())
        } else {
            None
        };
        let status_message = if ffmpeg_status.available {
            if ffmpeg_status.bundled {
                Some(format!("FFmpeg (同梱版) 使用中: {}", ffmpeg_status.path.display()))
            } else {
                Some(format!("FFmpeg (PATH) 使用中: {}", ffmpeg_status.path.display()))
            }
        } else {
            None
        };

        Self {
            project:    None,
            video_path: None,
            current_frame:   0,
            playing:         false,
            _last_play_tick: None,
            preview_texture: None,
            loaded_frame:    None,
            preview_scale:   PreviewScale::Auto,
            decode_tx:            req_tx,
            decode_rx:            res_rx,
            pending_decode_frame: None,
            selected_mosaic_id: None,
            drag_state: DragState::None,
            undo_stack: UndoStack::new(),
            mosaic_counter: 0,
            exporting:       false,
            export_progress: 0.0,
            export_rx:       None,
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
                self.video_path      = Some(path);
                self.project         = Some(project);
                self.current_frame   = 0;
                self.preview_texture = None;
                self.loaded_frame    = None;
                self.pending_decode_frame = None;
                self.selected_mosaic_id   = None;
                self.mosaic_counter  = 0;
                self.undo_stack      = UndoStack::new();
                self.error_message   = None;
                self.status_message  = Some("動画を読み込みました".to_string());
            }
            Err(e) => {
                self.error_message = Some(format!("動画を読み込めませんでした。\n\n{}", e));
            }
        }
    }

    /// バックグラウンドデコーダの結果をチェックし、テクスチャを更新する。
    /// UIスレッドをブロックしない (try_recv で即リターン)。
    fn update_preview_texture(&mut self, ctx: &Context) {
        let Some(_) = &self.video_path  else { return };
        let Some(_) = &self.project     else { return };

        // 完了したデコード結果を取得してテクスチャに反映
        // フレーム番号の厳密一致は不要: 再生中は常に最新デコード結果を表示する
        loop {
            match self.decode_rx.try_recv() {
                Ok(result) => {
                    let image = egui::ColorImage::from_rgb(
                        [result.pw as usize, result.ph as usize],
                        &result.pixels,
                    );
                    self.preview_texture = Some(ctx.load_texture(
                        "preview", image, TextureOptions::LINEAR,
                    ));
                    self.loaded_frame         = Some(result.frame);
                    self.pending_decode_frame = None;
                    // 再生中: タイムラインをデコード済みフレームに同期
                    if self.playing {
                        self.current_frame = result.frame;
                    }
                }
                Err(TryRecvError::Empty)        => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        // 次にデコードするフレームを決定
        // 再生中: loaded_frame の次を連続デコード (sequential)
        // 停止中: 現在のタイムライン位置
        let target_frame = if self.playing {
            match self.loaded_frame {
                Some(f) => {
                    let next = f + 1;
                    if next >= self.total_frames() {
                        // 末尾に達したら停止
                        self.playing = false;
                        return;
                    }
                    next
                }
                None => self.current_frame,
            }
        } else {
            self.current_frame
        };

        // 必要なら新しいデコードリクエストを送信
        if self.loaded_frame != Some(target_frame)
            && self.pending_decode_frame != Some(target_frame)
        {
            if let (Some(path), Some(proj)) = (&self.video_path, &self.project) {
                let scale = self.preview_scale.factor(proj.video.width, proj.video.height);
                let _ = self.decode_tx.send(Some(DecodeRequest {
                    path:   path.clone(),
                    frame:  target_frame,
                    fps:    proj.video.fps,
                    width:  proj.video.width,
                    height: proj.video.height,
                    scale,
                }));
                self.pending_decode_frame = Some(target_frame);
            }
        }

        // デコード待ち中は定期的に再描画してフレーム到着を拾う
        if self.pending_decode_frame.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn seek_to(&mut self, frame: u64) {
        let total = self.total_frames();
        if total == 0 { return; }
        self.current_frame   = frame.min(total - 1);
        self.loaded_frame    = None;
    }

    fn frame_step(&mut self, delta: i64) {
        let total = self.total_frames();
        if total == 0 { return; }
        let new_frame = (self.current_frame as i64 + delta).clamp(0, total as i64 - 1) as u64;
        self.current_frame = new_frame;
        self.loaded_frame  = None;
    }

    fn toggle_play(&mut self) {
        self.playing = !self.playing;
    }

    fn tick_playback(&mut self) {
        // フレーム進行は update_preview_texture 内のデコード完了で行う
        // 壁時計ベースの進行だとデコードが追いつかず結果が常に破棄される
    }

    fn handle_keyboard(&mut self, ctx: &Context) {
        let (shift, ctrl, keys_pressed) = ctx.input(|i| {
            let shift = i.modifiers.shift;
            let ctrl  = i.modifiers.command_only();
            let keys: Vec<Key> = [
                Key::Z, Key::Y, Key::Q, Key::E, Key::R,
                Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight,
                Key::Delete, Key::Space, Key::Home, Key::End,
            ]
            .iter()
            .filter(|&&k| i.key_pressed(k))
            .copied()
            .collect();
            (shift, ctrl, keys)
        });

        // Undo/Redo
        if ctrl {
            if keys_pressed.contains(&Key::Z) {
                if let Some(proj) = self.project.as_mut() {
                    self.undo_stack.undo(&mut proj.mosaics, &mut proj.export);
                }
                return;
            }
            if keys_pressed.contains(&Key::Y)
                || (shift && keys_pressed.contains(&Key::Z))
            {
                if let Some(proj) = self.project.as_mut() {
                    self.undo_stack.redo(&mut proj.mosaics, &mut proj.export);
                }
                return;
            }
        }

        // モザイク選択中のキー操作
        if let Some(id) = self.selected_mosaic_id.clone() {
            if let Some(proj) = &mut self.project {
                if let Some(region_idx) = proj.mosaics.iter().position(|m| m.id == id) {
                    let current = self.current_frame;
                    let before  = proj.mosaics[region_idx].clone();
                    let mut changed = false;

                    {
                        let region = &mut proj.mosaics[region_idx];
                        region.ensure_keyframe_at(current);
                        if let Some(kf) = region.keyframes.iter_mut().find(|k| k.frame == current) {
                            let step = if shift { 5.0 } else { 1.0 };
                            if keys_pressed.contains(&Key::Q) { kf.rotation_deg -= step; changed = true; }
                            if keys_pressed.contains(&Key::E) { kf.rotation_deg += step; changed = true; }
                            if keys_pressed.contains(&Key::R) { kf.rotation_deg  = 0.0;  changed = true; }

                            let move_step = if shift { 10.0 } else { 1.0 };
                            if keys_pressed.contains(&Key::ArrowUp)    { kf.center_y -= move_step; changed = true; }
                            if keys_pressed.contains(&Key::ArrowDown)  { kf.center_y += move_step; changed = true; }
                            if keys_pressed.contains(&Key::ArrowLeft)  { kf.center_x -= move_step; changed = true; }
                            if keys_pressed.contains(&Key::ArrowRight) { kf.center_x += move_step; changed = true; }
                        }
                    }

                    if keys_pressed.contains(&Key::Delete) {
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

        if keys_pressed.contains(&Key::Space) {
            self.toggle_play();
        }

        if self.selected_mosaic_id.is_none() {
            if keys_pressed.contains(&Key::ArrowLeft)  { if shift { self.frame_step(-10); } else { self.frame_step(-1); } }
            if keys_pressed.contains(&Key::ArrowRight) { if shift { self.frame_step(10);  } else { self.frame_step(1);  } }
            if keys_pressed.contains(&Key::Home) { self.seek_to(0); }
            if keys_pressed.contains(&Key::End)  {
                let total = self.total_frames();
                if total > 0 { self.seek_to(total - 1); }
            }
        }
    }

    fn add_mosaic(&mut self, cx: f32, cy: f32, w: f32, h: f32) {
        let Some(proj) = &mut self.project else { return };
        self.mosaic_counter += 1;
        let id   = format!("mosaic_{:04}", self.mosaic_counter);
        let name = format!("Mosaic {}", self.mosaic_counter);
        let kf   = MosaicKeyframe {
            frame: self.current_frame,
            center_x: cx, center_y: cy, width: w, height: h, rotation_deg: 0.0,
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
        let Some(proj) = &self.project    else { return };
        let Some(vp)   = &self.video_path else { return };
        let stem = vp.file_stem().unwrap_or_default().to_string_lossy();
        let dir  = vp.parent().unwrap_or(Path::new("."));
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
                        // カウンタを既存モザイク数以上に設定 (ID衝突防止)
                        self.mosaic_counter = proj.mosaics.len() as u32;
                        self.video_path      = Some(video_path);
                        self.project         = Some(proj);
                        self.current_frame   = 0;
                        self.loaded_frame    = None;
                        self.pending_decode_frame = None;
                        self.preview_texture = None;
                        self.status_message  = Some("プロジェクトを読み込みました".to_string());
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
        let Some(proj) = &self.project    else { return };
        let Some(vp)   = &self.video_path else { return };

        let total    = proj.video.total_frames;
        let trim_sum = proj.export.trim_start_frames + proj.export.trim_end_frames;
        if trim_sum >= total {
            self.error_message = Some(
                "除去フレーム数が動画の総フレーム数以上です。\n設定を確認してください。".to_string()
            );
            return;
        }

        let output_path = make_output_path(vp);
        let video_path  = vp.clone();
        let proj_clone  = proj.clone();

        self.exporting       = true;
        self.export_progress = 0.0;
        self.status_message  = Some("エクスポート中...".to_string());

        let (tx, rx) = crossbeam_channel::bounded::<ExportMsg>(64);
        std::thread::spawn(move || {
            run_export(&video_path, &output_path, &proj_clone, tx);
        });
        self.export_rx = Some(rx); // チャンネルを保持してポーリングする
    }

    /// エクスポート進捗チャンネルをポーリングしてUIを更新する
    fn poll_export(&mut self, ctx: &Context) {
        if !self.exporting { return; }

        let mut finished = false;
        if let Some(rx) = &self.export_rx {
            loop {
                match rx.try_recv() {
                    Ok(ExportMsg::Progress(cur, total)) => {
                        self.export_progress = if total > 0 { cur as f32 / total as f32 } else { 0.0 };
                    }
                    Ok(ExportMsg::Done) => {
                        self.status_message = Some("エクスポート完了".to_string());
                        finished = true;
                        break;
                    }
                    Ok(ExportMsg::Error(e)) => {
                        self.error_message = Some(format!("エクスポートエラー: {}", e));
                        finished = true;
                        break;
                    }
                    Err(TryRecvError::Empty)        => break,
                    Err(TryRecvError::Disconnected) => { finished = true; break; }
                }
            }
        }

        if finished {
            self.exporting   = false;
            self.export_rx   = None;
        } else {
            // 完了前は 100ms ごとに再描画して進捗を拾う
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

// ---- エクスポートワーカー ---------------------------------------------------

fn run_export(
    video_path:  &Path,
    output_path: &Path,
    proj:        &crate::model::Project,
    tx:          crossbeam_channel::Sender<ExportMsg>,
) {
    use std::io::Write;

    let fps         = proj.video.fps;
    let w           = proj.video.width;
    let h           = proj.video.height;
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
        Ok(c) => c,
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
                let regions: Vec<&MosaicRegion> = proj.mosaics.iter().collect();
                apply_mosaics(&mut pixels, w, h, &regions, frame_idx);
                if let Err(e) = stdin.write_all(&pixels) {
                    let _ = tx.send(ExportMsg::Error(format!("フレーム書き込み失敗: {}", e)));
                    return;
                }
            }
            Err(e) => {
                log::warn!("フレーム {} デコード失敗: {}", frame_idx, e);
                let blank = vec![0u8; (w * h * 3) as usize];
                let _ = stdin.write_all(&blank);
            }
        }
    }

    drop(stdin);

    match enc.wait() {
        Ok(s) if s.success() => { let _ = tx.send(ExportMsg::Done); }
        Ok(s) => { let _ = tx.send(ExportMsg::Error(format!("ffmpegがエラーで終了: {:?}", s))); }
        Err(e) => { let _ = tx.send(ExportMsg::Error(format!("ffmpeg待機失敗: {}", e))); }
    }
}

// ---- eframe::App -----------------------------------------------------------

impl eframe::App for LvMosaicApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // 再生ティック (フレームカウンタを実時間に合わせて進める)
        self.tick_playback();
        if self.playing {
            // 再生中は 33ms ごとに再描画 (≈30fps)
            ctx.request_repaint_after(Duration::from_millis(33));
        }

        // エクスポート進捗ポーリング
        self.poll_export(ctx);

        // キーボード処理
        if self.project.is_some() {
            self.handle_keyboard(ctx);
        }

        // プレビューフレーム更新 (バックグラウンドデコーダの結果確認)
        if self.project.is_some() {
            self.update_preview_texture(ctx);
        }

        // ドラッグ＆ドロップ
        if !ctx.input(|i| i.raw.dropped_files.is_empty()) {
            let paths: Vec<PathBuf> = ctx.input(|i| {
                i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect()
            });
            if let Some(path) = paths.into_iter().next() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                match ext.as_str() {
                    "mp4"  => self.load_video(path),
                    "json" => self.load_project_json(&path.clone()),
                    _ => {}
                }
            }
        }

        // ---- トップバー ----
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
                        if ui.button("Save").clicked() { self.save_project(); }
                        if !self.exporting && ui.add(
                            egui::Button::new("Export").fill(Color32::from_rgb(50, 120, 50))
                        ).clicked() {
                            self.start_export();
                        }
                    }
                });
            });

            // ステータスバー
            if let Some(msg) = &self.status_message {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(msg).small().color(Color32::GRAY));
                    if self.exporting {
                        ui.add(egui::ProgressBar::new(self.export_progress).desired_width(200.0));
                    }
                });
            }
        });

        // ---- 右側プロパティパネル ----
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

                    let current_frame    = self.current_frame;
                    let total_frames     = self.total_frames();
                    let exporting        = self.exporting;
                    let export_progress  = self.export_progress;

                    let result = if let Some(proj) = &mut self.project {
                        let selected = selected_idx.map(|i| &mut proj.mosaics[i]);
                        show_properties(
                            ui, selected, &mut proj.export,
                            current_frame, total_frames,
                            exporting, export_progress,
                        )
                    } else {
                        crate::ui::properties::PropertiesResult {
                            delete_mosaic: false, add_keyframe: false,
                            delete_keyframe: false, export_requested: false,
                            save_requested: false, mosaic_changed: false,
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
                    if result.export_requested { self.start_export(); }
                    if result.save_requested   { self.save_project(); }
                });
            });

        // ---- 下部タイムライン ----
        egui::TopBottomPanel::bottom("timeline")
            .min_height(80.0)
            .max_height(200.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let play_label = if self.playing { "⏸" } else { "▶" };
                    if ui.button(play_label).clicked() { self.toggle_play(); }
                    if ui.button("⏹").clicked() { self.playing = false; self.seek_to(0); }
                    if ui.button("◀◀").clicked() { self.frame_step(-1); }
                    if ui.button("▶▶").clicked() { self.frame_step(1); }

                    ui.separator();

                    let total   = self.total_frames();
                    let fps     = self.fps();
                    let cur_sec = if fps > 0.0 { self.current_frame as f64 / fps } else { 0.0 };
                    let tot_sec = if fps > 0.0 { total as f64 / fps } else { 0.0 };

                    // デコード待ちインジケータ
                    if self.pending_decode_frame.is_some() {
                        ui.spinner();
                    }

                    ui.label(format!(
                        "フレーム: {} / {}   時間: {:.2}s / {:.2}s",
                        self.current_frame, total, cur_sec, tot_sec
                    ));
                });

                ui.separator();

                if self.project.is_some() {
                    let mosaics = self.project.as_ref().map(|p| p.mosaics.as_slice()).unwrap_or(&[]);
                    let result  = show_timeline(
                        ui, mosaics, self.current_frame, self.total_frames(),
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

        // ---- 中央プレビューエリア ----
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.project.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.label(egui::RichText::new("mp4動画をここにドラッグ＆ドロップ").size(20.0).color(Color32::GRAY));
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("または").color(Color32::DARK_GRAY));
                        ui.add_space(8.0);
                        if ui.button("動画を開く").clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .add_filter("動画ファイル", &["mp4"]).pick_file()
                            {
                                self.load_video(p);
                            }
                        }
                        ui.add_space(8.0);
                        if ui.button("プロジェクトを開く").clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .add_filter("lvMosaic プロジェクト", &["json"]).pick_file()
                            {
                                self.load_project_json(&p);
                            }
                        }
                    });
                });
            } else {
                let mosaics      = self.project.as_ref().map(|p| p.mosaics.as_slice()).unwrap_or(&[]);
                let w            = self.video_width();
                let h            = self.video_height();
                let current_frame = self.current_frame;
                let selected_id  = self.selected_mosaic_id.clone();

                let prev_result = show_preview(
                    ui,
                    self.preview_texture.as_ref(),
                    w, h, mosaics, current_frame,
                    selected_id.as_deref(),
                    &self.drag_state,
                );

                self.drag_state         = prev_result.drag_state;
                self.selected_mosaic_id = prev_result.selected_id;

                if let Some((cx, cy, mw, mh)) = prev_result.new_mosaic_rect {
                    self.add_mosaic(cx, cy, mw, mh);
                }

                for (id, vals) in prev_result.mosaic_updates {
                    if let Some(proj) = &mut self.project {
                        if let Some(region) = proj.mosaics.iter_mut().find(|m| m.id == id) {
                            region.ensure_keyframe_at(current_frame);
                            if let Some(kf) = region.keyframes.iter_mut().find(|k| k.frame == current_frame) {
                                kf.center_x    = vals.center_x;
                                kf.center_y    = vals.center_y;
                                kf.width       = vals.width;
                                kf.height      = vals.height;
                                kf.rotation_deg = vals.rotation_deg;
                            }
                        }
                    }
                }
            }

            // エラーダイアログ
            if let Some(msg) = self.error_message.clone() {
                let mut open = true;
                egui::Window::new("エラー")
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label(&msg);
                        if ui.button("閉じる").clicked() {
                            self.error_message = None;
                        }
                    });
                if !open { self.error_message = None; }
            }
        });
    }
}
