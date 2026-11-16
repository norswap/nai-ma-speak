#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod config;
mod focus;
mod layout;
mod strip;
mod text;
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

/// System fonts with Chinese glyphs, in order of preference, each with the family to pick
/// from a font collection. Traditional Chinese (Taiwan) faces come first.
const CJK_FONTS: &[(&str, &str)] = &[
    (r"C:\Windows\Fonts\msjh.ttc", "Microsoft JhengHei"),
    (r"C:\Windows\Fonts\msyh.ttc", "Microsoft YaHei"),
    (r"C:\Windows\Fonts\msyh.ttf", ""),
    (r"C:\Windows\Fonts\simhei.ttf", ""),
    (r"C:\Windows\Fonts\simsun.ttc", ""),
    ("/System/Library/Fonts/PingFang.ttc", "PingFang TC"),
    // Current macOS has no PingFang.ttc there. Its PingFang (PingFangUI.ttc, in a private
    // framework) stores glyphs in Apple's `hvgl` table, which egui cannot draw: every
    // character comes out blank. Heiti has ordinary outlines.
    ("/System/Library/Fonts/STHeiti Medium.ttc", "Heiti TC"),
    ("/System/Library/Fonts/STHeiti Light.ttc", "Heiti TC"),
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", ""),
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", "Noto Sans CJK TC"),
    ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", "Noto Sans CJK TC"),
    ("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc", ""),
];

/// egui ships without Chinese glyphs; borrow a system font if one exists.
fn add_cjk_font(ctx: &egui::Context) {
    let found = CJK_FONTS.iter().find_map(|(path, family)| {
        let bytes = std::fs::read(path).ok()?;
        let index = face_index(&bytes, family).unwrap_or(0);
        has_outlines(&bytes, index).then_some((path, index, bytes))
    });
    let Some((path, index, bytes)) = found else {
        eprintln!("No usable Chinese system font found; Chinese text will show as empty boxes.");
        let mut fonts = egui::FontDefinitions::default();
        add_overlay_family(&mut fonts);
        ctx.set_fonts(fonts);
        return;
    };
    eprintln!("Chinese font: {path} (face {index})");
    let mut fonts = egui::FontDefinitions::default();
    let data = egui::FontData { index, ..egui::FontData::from_owned(bytes) };
    fonts.font_data.insert("cjk".into(), Arc::new(data));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("cjk".into());
    }
    add_overlay_family(&mut fonts);
    ctx.set_fonts(fonts);
}

/// Name of the font family the overlay draws with.
pub const OVERLAY_FAMILY: &str = "overlay";

/// How much smaller the overlay draws Western text than Chinese at the same font size.
/// Latin letters fill less of their height than Chinese characters and still read well
/// small, while Chinese needs the full size to stay legible.
const OVERLAY_WESTERN_SCALE: f32 = 0.85;

/// Adds the overlay's font family: the proportional fonts with every font but the Chinese
/// one scaled down. Without it the family would not exist and drawing with it would panic,
/// so this runs whether or not a Chinese font was found.
fn add_overlay_family(fonts: &mut egui::FontDefinitions) {
    let proportional = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    let names = proportional
        .iter()
        .map(|name| {
            if name == "cjk" {
                return name.clone();
            }
            let Some(font) = fonts.font_data.get(name) else {
                return name.clone();
            };
            let mut small = (**font).clone();
            small.tweak.scale *= OVERLAY_WESTERN_SCALE;
            let small_name = format!("{name}-overlay");
            fonts.font_data.insert(small_name.clone(), Arc::new(small));
            small_name
        })
        .collect();
    fonts.families.insert(egui::FontFamily::Name(OVERLAY_FAMILY.into()), names);
}

fn u16_at(font: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_be_bytes(font.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(font: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_be_bytes(font.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Number of faces in a font file: several in a collection (`.ttc`), otherwise one.
fn face_count(font: &[u8]) -> usize {
    if font.get(..4) == Some(b"ttcf") { u32_at(font, 8).unwrap_or(0) } else { 1 }
}

/// Offset of a table in one face of a font file.
fn table(font: &[u8], face: usize, tag: &[u8; 4]) -> Option<usize> {
    let font_at = if font.get(..4)? == b"ttcf" { u32_at(font, 12 + 4 * face)? } else { 0 };
    let record = (0..u16_at(font, font_at + 4)?)
        .map(|i| font_at + 12 + 16 * i)
        .find(|&record| font.get(record..record + 4) == Some(tag))?;
    u32_at(font, record + 8)
}

/// Whether a face stores its glyphs as TrueType or CFF outlines, the kinds egui can draw.
/// Apple's newer system fonts use an `hvgl` table instead; egui finds their characters but
/// draws nothing.
fn has_outlines(font: &[u8], face: u32) -> bool {
    [b"glyf", b"CFF ", b"CFF2"].iter().any(|tag| table(font, face as usize, tag).is_some())
}

/// Finds the face of a font collection (`.ttc`) whose family name is exactly `family`.
/// A collection holds several fonts, and face 0 is often not the wanted one: in PingFang it
/// is the Simplified Chinese face.
fn face_index(font: &[u8], family: &str) -> Option<u32> {
    if family.is_empty() || font.get(..4)? != b"ttcf" {
        return None;
    }
    (0..face_count(font))
        .find(|&face| {
            let family_matches = || {
                let name_at = table(font, face, b"name")?;
                let strings_at = name_at + u16_at(font, name_at + 4)?;
                Some((0..u16_at(font, name_at + 2)?).any(|i| {
                    let record = name_at + 6 + 12 * i;
                    // Windows platform, family name (name ID 1), stored as UTF-16BE.
                    let fields = [0, 6, 8, 10].map(|field| u16_at(font, record + field));
                    let [Some(3), Some(1), Some(length), Some(offset)] = fields else {
                        return false;
                    };
                    font.get(strings_at + offset..strings_at + offset + length).is_some_and(|bytes| {
                        let units = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_be_bytes(*pair));
                        char::decode_utf16(units).map(|c| c.unwrap_or('\u{fffd}')).eq(family.chars())
                    })
                }))
            };
            family_matches() == Some(true)
        })
        .map(|face| face as u32)
}

/// Prints egui and eframe warnings to stderr, e.g. a lost viewport or a full
/// font atlas, so a misbehaving session leaves a trace in the terminal.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}: {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

static LOG: StderrLog = StderrLog;

fn main() -> eframe::Result {
    if log::set_logger(&LOG).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    // `translate-companion --decode shot.png` reads a strip from a screenshot.
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--decode" {
        decode_file(&args[2]);
        return Ok(());
    }
    let config = Arc::new(Mutex::new(config::load()));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Translate companion")
            .with_inner_size([app::MIN_WIDTH, app::MIN_HEIGHT])
            .with_min_inner_size([app::MIN_WIDTH, app::MIN_HEIGHT])
            .with_icon(window_icon())
            // The renderer only gets an alpha channel when the root window asks
            // for one; without it the overlay is drawn solid black.
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "Translate companion",
        options,
        Box::new(move |cc| {
            add_cjk_font(&cc.egui_ctx);
            let (ui_tx, ui_rx) = mpsc::channel();
            let ui = worker::UiSender::new(ui_tx, cc.egui_ctx.clone());
            let (clip_tx, clip_rx) = mpsc::channel();
            let (strip_tx, strip_rx) = mpsc::channel();
            let (interactive_tx, interactive_rx) = mpsc::channel();

            worker::spawn(config.clone(), strip_rx, ui.clone(), clip_tx.clone());
            worker::spawn(config.clone(), interactive_rx, ui.clone(), clip_tx);
            strip::spawn(strip_tx, ui.clone());
            focus::spawn(ui.clone());
            clipboard::spawn(clip_rx, interactive_tx.clone(), ui);

            Ok(Box::new(app::TranslateApp::new(config, ui_rx, interactive_tx)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-face collection with just the tables `face_index` reads.
    fn collection(families: [&str; 2]) -> Vec<u8> {
        let mut out = b"ttcf".to_vec();
        out.extend([0, 1, 0, 0, 0, 0, 0, 2]);
        let header = out.len() + 8;
        let fonts: Vec<Vec<u8>> = families
            .iter()
            .map(|family| {
                let text: Vec<u8> = family.encode_utf16().flat_map(u16::to_be_bytes).collect();
                // Offset table with one table record, then the name table at +28.
                let mut font = vec![0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
                font.extend(b"name");
                font.extend([0; 12]);
                font.extend([0, 0, 0, 1, 0, 18]);
                font.extend([0, 3, 0, 1, 4, 9, 0, 1]);
                font.extend((text.len() as u16).to_be_bytes());
                font.extend([0, 0]);
                font.extend(text);
                font
            })
            .collect();
        let mut at = header;
        for font in &fonts {
            out.extend((at as u32).to_be_bytes());
            at += font.len();
        }
        let mut at = header;
        for mut font in fonts {
            let name_at = (at + 28) as u32;
            font[20..24].copy_from_slice(&name_at.to_be_bytes());
            at += font.len();
            out.extend(font);
        }
        out
    }

    #[test]
    fn picks_the_traditional_face_of_a_collection() {
        let font = collection(["PingFang SC", "PingFang TC"]);
        assert_eq!(face_index(&font, "PingFang TC"), Some(1));
        assert_eq!(face_index(&font, "PingFang SC"), Some(0));
        assert_eq!(face_index(&font, "PingFang HK"), None);
        assert_eq!(face_index(&font, ""), None);
        assert_eq!(face_index(b"not a font collection", "PingFang TC"), None);
        assert_eq!(face_index(&font[..40], "PingFang TC"), None);
    }

    #[test]
    fn only_accepts_faces_with_drawable_outlines() {
        let mut font = collection(["PingFang SC", "PingFang TC"]);
        assert!(!has_outlines(&font, 1), "a face with only a name table has no outlines");
        let tag = font.windows(4).rposition(|tag| tag == b"name").unwrap();
        font[tag..tag + 4].copy_from_slice(b"glyf");
        assert!(has_outlines(&font, 1));
        assert!(!has_outlines(&font, 0));
        assert!(!has_outlines(&font, 7));
    }

    /// Whether `font` puts ink in the font atlas for a Chinese character, or `None` if it
    /// has no glyph for it.
    fn draws_chinese(font: egui::FontData) -> Option<bool> {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("cjk".into(), Arc::new(font));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().push("cjk".into());
        ctx.set_fonts(fonts);
        let font_id = egui::FontId::proportional(32.0);
        let mut drawn = None;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            if ui.fonts_mut(|fonts| fonts.has_glyph(&font_id, '\u{9AD4}')) {
                let galley = ui.painter().layout_no_wrap("\u{9AD4}".into(), font_id.clone(), egui::Color32::WHITE);
                let mut glyphs = galley.rows.iter().flat_map(|row| &row.glyphs);
                drawn = Some(glyphs.any(|glyph| glyph.uv_rect.size.x > 0.0 && glyph.uv_rect.size.y > 0.0));
            }
        });
        output.textures_delta.clear();
        drawn
    }

    /// On a machine with one of the listed fonts, the one the companion would load must
    /// really draw Chinese, not only claim to have the characters.
    #[test]
    fn the_chosen_system_font_draws_chinese() {
        for (path, family) in CJK_FONTS {
            let Ok(bytes) = std::fs::read(path) else { continue };
            let index = face_index(&bytes, family).unwrap_or(0);
            if has_outlines(&bytes, index) {
                let font = egui::FontData { index, ..egui::FontData::from_owned(bytes) };
                assert_eq!(draws_chinese(font), Some(true), "{path} face {index}");
            }
        }
    }

    #[test]
    fn overlay_family_shrinks_western_text_only() {
        let ctx = egui::Context::default();
        add_cjk_font(&ctx);
        let size = 20.0;
        let normal = egui::FontId::new(size, egui::FontFamily::Proportional);
        let overlay = egui::FontId::new(size, egui::FontFamily::Name(OVERLAY_FAMILY.into()));
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let width = |text: &str, font: &egui::FontId| {
                ui.painter().layout_no_wrap(text.into(), font.clone(), egui::Color32::WHITE).size().x
            };
            let ratio = width("Need a healer", &overlay) / width("Need a healer", &normal);
            assert!((ratio - OVERLAY_WESTERN_SCALE).abs() < 0.03, "Western ratio {ratio}");
            if ui.fonts_mut(|fonts| fonts.has_glyph(&normal, '\u{9AD4}')) {
                assert_eq!(width("\u{7E41}\u{9AD4}", &overlay), width("\u{7E41}\u{9AD4}", &normal));
            }
        });
        output.textures_delta.clear();
    }
}
