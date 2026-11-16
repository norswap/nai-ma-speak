#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod backends;
mod clipboard;
mod config;
mod diag;
mod focus;
mod fonts;
mod layout;
mod markup;
mod protocol;
mod strip;
#[cfg(test)]
mod testing;
mod text;
mod theme;
mod translate;
mod worker;

use eframe::egui;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// The title bar and taskbar icon, built into the binary.
fn window_icon() -> egui::IconData {
    let image = xcap::image::load_from_memory(include_bytes!("../icon.png"))
        .expect("icon.png is a valid PNG")
        .to_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData { rgba: image.into_raw(), width, height }
}

fn decode_file(path: &str) {
    let img = match xcap::image::open(path) {
        Ok(img) => img.to_rgba8(),
        Err(e) => return println!("Could not open {path}: {e}"),
    };
    match strip::find(&img) {
        None => println!("No pixel strip in {path}"),
        Some(lock) => println!("Strip at {},{} ({} px blocks): {:?}", lock.x, lock.y, lock.block, strip::decode(&img, lock)),
    }
}

fn main() -> eframe::Result {
    // `translate-companion --decode shot.png` reads a strip from a screenshot.
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--decode" {
        decode_file(&args[2]);
        return Ok(());
    }
    diag::start();
    let config = Arc::new(Mutex::new(config::load()));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Nǎi Mā Speak")
            .with_inner_size(app::START_SIZE)
            .with_min_inner_size([app::MIN_WIDTH, app::MIN_HEIGHT])
            .with_icon(window_icon())
            // The renderer only gets an alpha channel when the root window asks
            // for one; without it the overlay is drawn solid black.
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "Nai Ma Speak",
        options,
        Box::new(move |cc| {
            fonts::add(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            let (ui_tx, ui_rx) = mpsc::channel();
            let ui = worker::UiSender::new(ui_tx, cc.egui_ctx.clone());
            let (clip_tx, clip_rx) = mpsc::channel();
            let (strip_tx, strip_rx) = mpsc::channel();
            let (interactive_tx, interactive_rx) = mpsc::channel();

            worker::spawn(config.clone(), strip_rx, ui.clone(), clip_tx.clone());
            worker::spawn(config.clone(), interactive_rx, ui.clone(), clip_tx);
            strip::spawn(strip_tx, ui.clone());
            focus::spawn(ui.clone());
            clipboard::spawn(clip_rx, ui);

            Ok(Box::new(app::TranslateApp::new(config, ui_rx, interactive_tx)))
        }),
    )
}
