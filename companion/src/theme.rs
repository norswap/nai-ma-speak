//! The companion window's look: a warm dark panel with gold edges and serif headings,
//! after the game's classic interface. Colors and a font only; none of the game's art.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, RichText, Stroke, TextStyle};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x17, 0x12, 0x0d);
pub const BAR: Color32 = Color32::from_rgb(0x24, 0x1b, 0x11);
pub const FIELD: Color32 = Color32::from_rgb(0x0d, 0x0a, 0x07);
pub const BORDER: Color32 = Color32::from_rgb(0x8a, 0x6d, 0x3b);
pub const BORDER_DIM: Color32 = Color32::from_rgb(0x4d, 0x3d, 0x22);
pub const GOLD: Color32 = Color32::from_rgb(0xff, 0xd1, 0x00);
pub const TEXT: Color32 = Color32::from_rgb(0xf0, 0xe6, 0xd2);
pub const MUTED: Color32 = Color32::from_rgb(0xb8, 0xaa, 0x8c);
pub const BUTTON: Color32 = Color32::from_rgb(0x5a, 0x1a, 0x12);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(0x7a, 0x24, 0x18);
pub const GOOD: Color32 = Color32::from_rgb(0x4c, 0xc3, 0x8a);
pub const BAD: Color32 = Color32::from_rgb(0xff, 0xab, 0x91);

/// Name of the font family for headings, labels and buttons.
pub const HEADING_FAMILY: &str = "heading";

/// Marcellus, under the SIL Open Font License (see THIRD_PARTY_NOTICES).
const HEADING_FONT: &[u8] = include_bytes!("fonts/Marcellus-Regular.ttf");

/// Adds the heading family: Marcellus, then the proportional fonts for what it lacks.
pub fn add_heading_family(fonts: &mut egui::FontDefinitions) {
    fonts.font_data.insert("marcellus".into(), std::sync::Arc::new(egui::FontData::from_static(HEADING_FONT)));
    let mut names = vec!["marcellus".to_string()];
    names.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Name(HEADING_FAMILY.into()), names);
}

pub fn heading_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(HEADING_FAMILY.into()))
}

/// A gold label in the heading font, as above each setting.
pub fn label(text: &str) -> RichText {
    RichText::new(text).font(heading_font(14.0)).color(GOLD)
}

/// Sets the colors, corners and fonts of every widget. The heading family must exist.
pub fn apply(ctx: &egui::Context) {
    ctx.global_style_mut(|style| {
        style.text_styles.insert(TextStyle::Body, FontId::proportional(13.5));
        style.text_styles.insert(TextStyle::Button, heading_font(14.0));
        style.text_styles.insert(TextStyle::Heading, heading_font(20.0));
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.interact_size.y = 26.0;

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
        for (state, fill, edge) in [
            (&mut widgets.inactive, BUTTON, BORDER),
            (&mut widgets.hovered, BUTTON_HOVER, GOLD),
            (&mut widgets.active, BUTTON_HOVER, GOLD),
            (&mut widgets.open, BUTTON_HOVER, GOLD),
        ] {
            state.weak_bg_fill = fill;
            state.bg_fill = fill;
            state.bg_stroke = Stroke::new(1.0, edge);
            state.fg_stroke = Stroke::new(1.0, GOLD);
            state.corner_radius = corners;
        }
    });
}
