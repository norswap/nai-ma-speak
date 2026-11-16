//! The fonts: Arial for body text, Marcellus for headings, the game's chat font and a
//! system font with Chinese for the overlay.

use eframe::egui;
use std::sync::Arc;

/// Name of the font family for headings, labels and buttons.
pub const HEADING_FAMILY: &str = "heading";

/// Marcellus, under the SIL Open Font License (see THIRD_PARTY_NOTICES).
const HEADING_FONT: &[u8] = include_bytes!("fonts/Marcellus-Regular.ttf");

/// Arial, where systems keep it. It is drawn for small sizes on ordinary screens, where
/// egui's own thin font turns ragged.
const BODY_FONTS: &[&str] = &["/System/Library/Fonts/Supplemental/Arial.ttf", "C:\\Windows\\Fonts\\arial.ttf"];

/// Draws each letter on whole pixels. egui otherwise also keeps copies shifted by parts
/// of a pixel, which spaces letters more evenly but blurs them on ordinary screens.
fn crisp(mut font: egui::FontData) -> egui::FontData {
    font.tweak.subpixel_binning = Some(false);
    font
}

/// Puts Arial first among the proportional fonts, if the system has it.
fn add_body_font(fonts: &mut egui::FontDefinitions) {
    let Some(bytes) = BODY_FONTS.iter().find_map(|path| std::fs::read(path).ok()) else {
        return;
    };
    fonts.font_data.insert("body".into(), Arc::new(crisp(egui::FontData::from_owned(bytes))));
    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "body".into());
}

/// Adds the heading family: Marcellus, then the proportional fonts for what it lacks.
fn add_heading_family(fonts: &mut egui::FontDefinitions) {
    fonts.font_data.insert("marcellus".into(), Arc::new(crisp(egui::FontData::from_static(HEADING_FONT))));
    let mut names = vec!["marcellus".to_string()];
    names.extend(fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(egui::FontFamily::Name(HEADING_FAMILY.into()), names);
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

/// Sets up the fonts: Arial for body text, a system font for Chinese (egui ships without
/// Chinese glyphs), and the overlay's and the headings' families.
pub fn add(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    add_body_font(&mut fonts);
    let found = CJK_FONTS.iter().find_map(|(path, family)| {
        let bytes = std::fs::read(path).ok()?;
        let index = face_index(&bytes, family).unwrap_or(0);
        has_outlines(&bytes, index).then_some((path, index, bytes))
    });
    match found {
        Some((path, index, bytes)) => {
            eprintln!("Chinese font: {path} (face {index})");
            let data = egui::FontData { index, ..egui::FontData::from_owned(bytes) };
            fonts.font_data.insert("cjk".into(), Arc::new(data));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("cjk".into());
            }
        }
        None => eprintln!("No usable Chinese system font found; Chinese text will show as empty boxes."),
    }
    add_overlay_family(&mut fonts);
    add_heading_family(&mut fonts);
    ctx.set_fonts(fonts);
}

/// Name of the font family the overlay draws with.
pub const OVERLAY_FAMILY: &str = "overlay";

/// How much smaller the overlay draws egui's own Western fonts than Chinese at the same
/// font size. Their letters fill less of their height than Chinese characters and still
/// read well small, while Chinese needs the full size to stay legible.
const OVERLAY_WESTERN_SCALE: f32 = 0.85;

/// Arial Narrow, the font the English game client draws chat with, where systems keep it.
const CHAT_FONTS: &[&str] = &["/System/Library/Fonts/Supplemental/Arial Narrow.ttf", "C:\\Windows\\Fonts\\ARIALN.TTF"];

fn chat_font() -> Option<Vec<u8>> {
    CHAT_FONTS.iter().find_map(|path| std::fs::read(path).ok())
}

/// Adds the overlay's font family: the game's chat font first, at the size asked for, so
/// Western text looks as it does in the game's chat; then the Chinese font; then egui's
/// own fonts, scaled down, for what those two lack. Without the family drawing with it
/// would panic, so this runs whether or not the system fonts were found.
fn add_overlay_family(fonts: &mut egui::FontDefinitions) {
    let proportional = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    let chat = chat_font().map(|bytes| {
        fonts.font_data.insert("chat".into(), Arc::new(crisp(egui::FontData::from_owned(bytes))));
        "chat".to_string()
    });
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
        .collect::<Vec<_>>();
    let names = chat.into_iter().chain(names).collect();
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
    fn overlay_family_uses_the_chat_font_or_shrinks_western_text() {
        let ctx = egui::Context::default();
        add(&ctx);
        let size = 20.0;
        let normal = egui::FontId::new(size, egui::FontFamily::Proportional);
        let overlay = egui::FontId::new(size, egui::FontFamily::Name(OVERLAY_FAMILY.into()));
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let width = |text: &str, font: &egui::FontId| {
                ui.painter().layout_no_wrap(text.into(), font.clone(), egui::Color32::WHITE).size().x
            };
            let ratio = width("Need a healer", &overlay) / width("Need a healer", &normal);
            if chat_font().is_some() {
                // Arial Narrow at full size is still narrower than egui's font at full size.
                assert!(ratio < 0.95 && (ratio - OVERLAY_WESTERN_SCALE).abs() > 0.005, "chat font ratio {ratio}");
            } else {
                assert!((ratio - OVERLAY_WESTERN_SCALE).abs() < 0.03, "Western ratio {ratio}");
            }
            if ui.fonts_mut(|fonts| fonts.has_glyph(&normal, '\u{9AD4}')) {
                assert_eq!(width("\u{7E41}\u{9AD4}", &overlay), width("\u{7E41}\u{9AD4}", &normal));
            }
        });
        output.textures_delta.clear();
    }
}
