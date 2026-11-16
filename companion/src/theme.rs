//! The companion window's look: a warm dark panel with gold edges and serif headings,
//! after the game's classic interface. Colors and a font only; none of the game's art.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, RichText, Stroke, TextStyle};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x17, 0x12, 0x0d);
pub const BAR: Color32 = Color32::from_rgb(0x24, 0x1b, 0x11);
pub const FIELD: Color32 = Color32::from_rgb(0x0d, 0x0a, 0x07);
pub const BORDER: Color32 = Color32::from_rgb(0x8a, 0x6d, 0x3b);
pub const BORDER_DIM: Color32 = Color32::from_rgb(0x4d, 0x3d, 0x22);
pub const BORDER_FIELD: Color32 = Color32::from_rgb(0x6b, 0x56, 0x30);
pub const BORDER_BRIGHT: Color32 = Color32::from_rgb(0xc9, 0xa2, 0x4d);
pub const GOLD: Color32 = Color32::from_rgb(0xff, 0xd1, 0x00);
pub const TEXT: Color32 = Color32::from_rgb(0xf0, 0xe6, 0xd2);
pub const MUTED: Color32 = Color32::from_rgb(0xb8, 0xaa, 0x8c);
pub const BUTTON: Color32 = Color32::from_rgb(0x5a, 0x1a, 0x12);
pub const GOOD: Color32 = Color32::from_rgb(0x4c, 0xc3, 0x8a);
pub const BAD: Color32 = Color32::from_rgb(0xff, 0xab, 0x91);

/// Name of the font family for headings, labels and buttons.
pub const HEADING_FAMILY: &str = "heading";

/// Marcellus, under the SIL Open Font License (see THIRD_PARTY_NOTICES).
const HEADING_FONT: &[u8] = include_bytes!("fonts/Marcellus-Regular.ttf");

/// Arial, where systems keep it. It is drawn for small sizes on ordinary screens, where
/// egui's own thin font turns ragged.
const BODY_FONTS: &[&str] = &["/System/Library/Fonts/Supplemental/Arial.ttf", "C:\\Windows\\Fonts\\arial.ttf"];

/// Draws each letter on whole pixels. egui otherwise also keeps copies shifted by parts
/// of a pixel, which spaces letters more evenly but blurs them on ordinary screens.
pub fn crisp(mut font: egui::FontData) -> egui::FontData {
    font.tweak.subpixel_binning = Some(false);
    font
}

/// Puts Arial first among the proportional fonts, if the system has it.
pub fn add_body_font(fonts: &mut egui::FontDefinitions) {
    let Some(bytes) = BODY_FONTS.iter().find_map(|path| std::fs::read(path).ok()) else {
        return;
    };
    fonts.font_data.insert("body".into(), std::sync::Arc::new(crisp(egui::FontData::from_owned(bytes))));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "body".into());
}

/// Adds the heading family: Marcellus, then the proportional fonts for what it lacks.
pub fn add_heading_family(fonts: &mut egui::FontDefinitions) {
    fonts.font_data.insert("marcellus".into(), std::sync::Arc::new(crisp(egui::FontData::from_static(HEADING_FONT))));
    let mut names = vec!["marcellus".to_string()];
    names.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Name(HEADING_FAMILY.into()), names);
}

pub fn heading_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(HEADING_FAMILY.into()))
}

/// A gold label in the heading font, as above each setting.
pub fn label(text: &str) -> RichText {
    RichText::new(text).font(heading_font(15.0)).color(GOLD)
}

/// A button as the game's classic interface draws them: dark red with a gold edge and
/// gold lettering.
pub fn button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).font(heading_font(15.0)).color(GOLD))
        .fill(BUTTON)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(4)
}

/// The one button a tab is there for, with a brighter edge.
pub fn main_button(text: &str) -> egui::Button<'static> {
    button(text).stroke(Stroke::new(1.5, BORDER_BRIGHT)).min_size(egui::vec2(120.0, 30.0))
}

/// A one-line text field: nearly black, with a bronze edge and room around the text.
pub fn field(text: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).margin(egui::Margin::symmetric(9, 6)).desired_width(f32::INFINITY)
}

/// Sets the colors, corners and fonts of every widget. The heading family must exist.
pub fn apply(ctx: &egui::Context) {
    ctx.global_style_mut(|style| {
        style.text_styles.insert(TextStyle::Body, FontId::proportional(14.0));
        // Drop-down lists and their entries use the button style; they read as fields.
        style.text_styles.insert(TextStyle::Button, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Heading, heading_font(21.0));
        style.spacing.item_spacing = egui::vec2(8.0, 7.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;

        let visuals = &mut style.visuals;
        *visuals = egui::Visuals::dark();
        visuals.panel_fill = BACKGROUND;
        visuals.window_fill = BACKGROUND;
        visuals.window_stroke = Stroke::new(1.0, BORDER);
        visuals.extreme_bg_color = FIELD;
        visuals.selection.bg_fill = Color32::from_rgb(0x6b, 0x56, 0x30);
        visuals.selection.stroke = Stroke::new(1.0, GOLD);
        visuals.hyperlink_color = GOLD;
        let corners = CornerRadius::same(4);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_DIM);
        widgets.noninteractive.corner_radius = corners;
        // Fields, drop-downs, checkboxes and sliders: dark with a bronze edge that turns
        // gold under the pointer. Buttons set their own dark red; see `button`.
        for (state, fill, edge) in [
            (&mut widgets.inactive, FIELD, BORDER_FIELD),
            (&mut widgets.hovered, BAR, BORDER_BRIGHT),
            (&mut widgets.active, BAR, GOLD),
            (&mut widgets.open, BAR, GOLD),
        ] {
            state.weak_bg_fill = fill;
            state.bg_fill = if fill == FIELD { BORDER_DIM } else { BORDER_FIELD };
            state.bg_stroke = Stroke::new(1.0, edge);
            state.fg_stroke = Stroke::new(1.0, TEXT);
            state.corner_radius = corners;
        }
    });
}
