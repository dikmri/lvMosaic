// リリースビルドではコンソールウィンドウを非表示にする
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod model;
mod mosaic;
mod ui;
mod undo;
mod video;

use app::LvMosaicApp;

fn main() -> eframe::Result {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("lvMosaic")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "lvMosaic",
        options,
        Box::new(|cc| Ok(Box::new(LvMosaicApp::new(cc)))),
    )
}
