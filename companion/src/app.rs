use crate::backends::claude::key_problem;
use crate::config::{self, Backend, Config, ReplyVariant, CLAUDE_MODELS, CUSTOM_LABEL, ENDPOINT_PRESETS};
use crate::translate::WOW_CHAT_BYTES;
use crate::focus::WowView;
use crate::layout::{self, ChatWindow};
use crate::markup::{self, ChatLine, Span};
use crate::worker::{Job, UiEvent};
use eframe::egui::text::LayoutJob;
use eframe::egui::{self, Color32, RichText, TextFormat, ViewportBuilder, ViewportCommand, ViewportId};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
const HIDE_LABEL: &str = "Hide when WoW is hidden or covered";
#[cfg(not(target_os = "macos"))]
const HIDE_LABEL: &str = "Hide when WoW is in the background";

/// How often the whole font texture is re-sent even without new text.
const FONT_TEXTURE_REFRESH: Duration = Duration::from_secs(5);

/// How many lines one mouse wheel step scrolls the overlay.
const WHEEL_LINES: f32 = 3.0;
/// How far the overlay text's shadow is offset, in points.
const SHADOW: f32 = 1.0;
/// How much of its brightness a Chinese line keeps while it waits for its translation.
const PENDING_DIM: f32 = 0.6;

struct OverlayLine {
    /// The number `UiEvent::Translated` refers to; 0 for lines that never get one.
    id: u64,
    line: ChatLine,
    /// The body in English, once it arrived.
    translated: Option<Vec<Span>>,
    /// The body is Chinese and its translation has not arrived yet.
    pending: bool,
    /// The translation was given up on; the body stays in Chinese.
    failed: bool,
}

impl OverlayLine {
    fn plain(prefix: String, body: String) -> Self {
        let line = ChatLine { color: [255; 3], prefix: vec![Span::plain(prefix)], body: vec![Span::plain(body)] };
        Self { id: 0, line, translated: None, pending: false, failed: false }
    }

    /// The line as the game words and colors it, with the body in English when there is one.
    fn job(&self, font: &egui::FontId, width: f32) -> LayoutJob {
        let mut job = LayoutJob::default();
        job.wrap.max_width = width;
        let body = self.translated.as_deref().unwrap_or(&self.line.body);
        let dim = if self.pending { PENDING_DIM } else { 1.0 };
        let parts = self.line.prefix.iter().map(|span| (span, 1.0)).chain(body.iter().map(|span| (span, dim)));
        for (span, brightness) in parts {
            let [r, g, b] = span.color.unwrap_or(self.line.color);
            let color = Color32::from_rgb(r, g, b).gamma_multiply(brightness);
            job.append(&span.text, 0.0, TextFormat::simple(font.clone(), color));
        }
        let note = match (self.pending, self.failed) {
            (_, true) => " (translation failed)",
            (true, _) => " (translation pending)",
            _ => "",
        };
        job.append(note, 0.0, TextFormat::simple(font.clone(), Color32::from_gray(150)));
        job
    }
}

pub struct TranslateApp {
    config: Arc<Mutex<Config>>,
    draft: Config,
    events: Receiver<UiEvent>,
    interactive: Sender<Job>,
    lines: VecDeque<OverlayLine>,
    status: String,
    notice: String,
    /// The last reply in Chinese, and whether it was copied; shown under the reply box.
    last_reply: Option<(String, bool)>,
    reply: String,
    locked: bool,
    overlay_start: egui::Pos2,
    overlay_rect: Option<egui::Rect>,
    test_result: String,
    strip_status: String,
    fitted_height: f32,
    wow_view: WowView,
    /// When the whole font texture was last sent; see `refresh_font_texture`.
    font_texture_sent: Instant,
    /// New overlay text arrived, so the font texture is re-sent after this frame.
    font_texture_stale: bool,
    chat_windows: Vec<ChatWindow>,
    /// Whether the overlay was on a chat tab last frame, and where it was last sent.
    overlay_anchored: bool,
    overlay_sent: Option<egui::Pos2>,
    overlay_hidden: bool,
    /// The settings show the fields for typing an endpoint, or a Claude model, by hand.
    custom_endpoint: bool,
    custom_claude_model: bool,
    /// The last mouse wheel count the addon reported for each chat window, and the steps
    /// over the overlay's tab that the overlay has not scrolled by yet.
    wheel_counts: HashMap<String, i64>,
    wheel_steps: f32,
}

/// Smallest companion window that shows every control with Settings closed.
pub const MIN_WIDTH: f32 = 560.0;
pub const MIN_HEIGHT: f32 = 150.0;

const OFF_SCREEN: f32 = -32000.0;

fn overlay_id() -> ViewportId {
    ViewportId::from_hash_of("overlay")
}

impl TranslateApp {
    pub fn new(config: Arc<Mutex<Config>>, events: Receiver<UiEvent>, interactive: Sender<Job>) -> Self {
        let draft = config.lock().unwrap().clone();
        let overlay_start = egui::pos2(draft.overlay_x, draft.overlay_y);
        Self {
            config,
            events,
            interactive,
            lines: VecDeque::new(),
            status: "Ready".into(),
            notice: String::new(),
            last_reply: None,
            reply: String::new(),
            locked: true,
            overlay_start,
            overlay_rect: None,
            fitted_height: 0.0,
            wow_view: WowView::Unknown,
            font_texture_sent: Instant::now(),
            font_texture_stale: false,
            chat_windows: Vec::new(),
            overlay_anchored: false,
            overlay_sent: None,
            overlay_hidden: false,
            custom_endpoint: draft.endpoint_preset().is_none(),
            custom_claude_model: draft.claude_preset().is_none(),
            wheel_counts: HashMap::new(),
            wheel_steps: 0.0,
            test_result: String::new(),
            strip_status: "Starting the pixel strip reader".into(),
            draft,
        }
    }

    fn push_line(&mut self, line: OverlayLine) {
        self.lines.push_back(line);
        while self.lines.len() > self.draft.max_lines.max(1) {
            self.lines.pop_front();
        }
        self.font_texture_stale = true;
    }

    /// Sends the whole font atlas to the GPU again, after new overlay text and
    /// every few seconds anyway.
    ///
    /// egui normally uploads only the glyphs added since the last frame. On
    /// macOS that stopped working once after some hours: every glyph
    /// rasterized from then on drew as fragments of other glyphs in both
    /// windows until the app was restarted. Atlas growth, light/dark theme
    /// switches, window resizes and overlay hiding did not reproduce it, so
    /// rather than guess at the trigger the companion re-sends the complete
    /// atlas, which repairs the texture whatever went wrong. The copy is a few
    /// megabytes, so it is cheap at this rate.
    fn refresh_font_texture(&mut self, ctx: &egui::Context) {
        let due = self.font_texture_stale || self.font_texture_sent.elapsed() >= FONT_TEXTURE_REFRESH;
        if !due {
            return;
        }
        let image = ctx.fonts(|fonts| fonts.image());
        let delta = egui::epaint::ImageDelta::full(image, egui::epaint::TextureAtlas::texture_options());
        ctx.tex_manager().write().set(egui::TextureId::default(), delta);
        if self.font_texture_stale {
            // The overlay was painted before this upload; show it again with the repaired texture.
            ctx.request_repaint();
        }
        self.font_texture_sent = Instant::now();
        self.font_texture_stale = false;
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                UiEvent::Line { id, line, chinese } => {
                    self.push_line(OverlayLine { id, line, translated: None, pending: chinese, failed: false });
                }
                UiEvent::Translated { id, english } => {
                    if let Some(line) = self.lines.iter_mut().rev().find(|line| line.id == id) {
                        line.translated = Some(markup::carry_colors(&english, &line.line.body));
                        line.pending = false;
                        self.font_texture_stale = true;
                    }
                }
                UiEvent::Failed { id } => {
                    if let Some(line) = self.lines.iter_mut().rev().find(|line| line.id == id) {
                        line.pending = false;
                        line.failed = true;
                    }
                }
                UiEvent::PasteReady { id, english } => {
                    self.notice = format!("#{id} ready: press Ctrl+V in the addon window");
                    self.push_line(OverlayLine::plain(format!("#{id}: "), english));
                }
                UiEvent::ReplyReady { english, chinese, copied } => {
                    self.notice = if copied {
                        format!("Copied \"{chinese}\" for \"{english}\". Paste it into WoW chat.")
                    } else {
                        format!("Not copied: \"{chinese}\" is too long for WoW chat. Say it in fewer words.")
                    };
                    self.last_reply = Some((chinese, copied));
                }
                UiEvent::TestResult { result, seconds } => {
                    let backend = self.config.lock().unwrap().backend.label();
                    self.test_result = match result {
                        Ok(english) if crate::text::has_chinese(&english) => {
                            format!("{backend} answered but did not translate ({seconds:.1} s): {english}")
                        }
                        Ok(english) => {
                            format!("{backend} works ({seconds:.1} s): {} -> {english}", crate::worker::TEST_LINE)
                        }
                        Err(e) => format!("{backend} test failed: {e}"),
                    };
                }
                UiEvent::Strip(text) => self.strip_status = text,
                UiEvent::WowView(view) => self.wow_view = view,
                UiEvent::ChatWindows(windows) => self.chat_windows = windows,
                UiEvent::Wheel { name, count } => {
                    // The first count only sets the starting point: its steps were made
                    // before the companion was watching.
                    let before = self.wheel_counts.insert(name.clone(), count);
                    let anchored = layout::anchor(&self.chat_windows, &self.draft.anchor_tab);
                    if let (Some(before), Some(window)) = (before, anchored) {
                        if window.name == name {
                            self.wheel_steps += (count - before) as f32;
                        }
                    }
                }
                UiEvent::Status(text) => self.status = text,
            }
        }
    }

    fn save(&mut self) {
        // While the overlay sits on a chat tab, overlay_rect keeps the hand-placed spot.
        if let Some(rect) = self.overlay_rect {
            self.draft.overlay_x = rect.min.x;
            self.draft.overlay_y = rect.min.y;
        }
        *self.config.lock().unwrap() = self.draft.clone();
        self.notice = match config::save(&self.draft) {
            Ok(()) => format!("Saved {}", config::path().display()),
            Err(e) => format!("Could not save settings: {e}"),
        };
    }

    fn control_panel(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let panel = egui::CentralPanel::default().show(root, |ui| {
            // Scrolls only when the screen is too short for the whole window.
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| self.control_contents(ui))
                .content_size
                .y
        });
        self.fit_window(&ctx, panel.inner);
    }

    /// Grows or shrinks the window to its contents, e.g. when Settings opens.
    fn fit_window(&mut self, ctx: &egui::Context, content_height: f32) {
        // Panel margins plus a few pixels so the scrollbar does not appear at an exact fit.
        let margin = 2.0 * ctx.global_style().spacing.window_margin.topf().max(8.0) + 6.0;
        let max = ctx
            .input(|i| i.viewport().monitor_size)
            .map_or(f32::INFINITY, |m| m.y * 0.9);
        let wanted = (content_height + margin).clamp(MIN_HEIGHT, max.max(MIN_HEIGHT));
        if (wanted - self.fitted_height).abs() < 1.0 {
            return;
        }
        self.fitted_height = wanted;
        let width = ctx.input(|i| i.viewport().inner_rect).map_or(MIN_WIDTH, |r| r.width());
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(width, wanted)));
    }

    fn control_contents(&mut self, ui: &mut egui::Ui) {
        {
            ui.heading("Translate companion");
            // Chat lines leave this computer for the service named here, unless it runs locally.
            ui.label(format!("{} · translating with {}", self.status, self.draft.backend.label()));
            if !self.notice.is_empty() {
                ui.colored_label(Color32::from_rgb(120, 200, 255), &self.notice);
            }
            ui.label(&self.strip_status);
            ui.horizontal(|ui| {
                if ui.button("Test translation").clicked() {
                    // Test what is typed in Settings, even before Save.
                    *self.config.lock().unwrap() = self.draft.clone();
                    let _ = self.interactive.send(Job::Test);
                    self.test_result = format!("Testing {}", self.draft.backend.label());
                }
                if !self.test_result.is_empty() {
                    ui.label(&self.test_result);
                }
            });
            ui.separator();

            ui.label("Reply in Chinese (Enter copies the translation for pasting into WoW chat):");
            let edit = ui.add(egui::TextEdit::singleline(&mut self.reply).desired_width(f32::INFINITY));
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.reply.trim().is_empty() {
                let _ = self.interactive.send(Job::Reply { text: self.reply.trim().to_string() });
                self.notice = "Translating your reply".into();
                self.last_reply = None;
                self.reply.clear();
                edit.request_focus();
            }
            // WoW's chat box holds 255 bytes, and a Chinese character takes 3 of them.
            let mut copy_anyway = false;
            if let Some((chinese, copied)) = &self.last_reply {
                ui.horizontal_wrapped(|ui| {
                    let size = format!("{}/{WOW_CHAT_BYTES} bytes", chinese.len());
                    if *copied {
                        ui.label(format!("{size}: {chinese}"));
                    } else {
                        ui.colored_label(Color32::from_rgb(255, 120, 90), format!("{size}, WoW would cut it off: {chinese}"));
                        copy_anyway = ui.button("Copy anyway").clicked();
                    }
                });
            }
            if copy_anyway {
                if let Some((chinese, copied)) = &mut self.last_reply {
                    ui.ctx().copy_text(chinese.clone());
                    *copied = true;
                    self.notice = "Copied the long reply. WoW will cut it at 255 bytes.".into();
                }
            }
            ui.separator();

            ui.horizontal(|ui| {
                let label = if self.locked { "Unlock overlay to move it" } else { "Lock overlay" };
                if ui.button(label).clicked() {
                    self.locked = !self.locked;
                    if self.locked {
                        self.save();
                    }
                }
                if ui.button("Clear overlay").clicked() {
                    self.lines.clear();
                }
                if ui.button("Restart").on_hover_text("Quit and start the companion again. Unsaved settings are lost.").clicked() {
                    match config::start_again() {
                        Ok(()) => ui.ctx().send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close),
                        Err(e) => self.notice = format!("Could not restart: {e}"),
                    }
                }
                ui.checkbox(&mut self.draft.show_original, "Show Chinese too");
                ui.checkbox(&mut self.draft.hide_when_unfocused, HIDE_LABEL);
            });

            egui::CollapsingHeader::new("Settings").default_open(false).show(ui, |ui| {
                egui::Grid::new("settings").num_columns(2).show(ui, |ui| {
                    ui.label("Translate with");
                    egui::ComboBox::from_id_salt("backend").selected_text(self.draft.backend.label()).show_ui(ui, |ui| {
                        for backend in Backend::ALL {
                            ui.selectable_value(&mut self.draft.backend, backend, backend.label());
                        }
                    });
                    ui.end_row();
                    match self.draft.backend {
                        Backend::GoogleFree => {
                            ui.label("");
                            ui.label("No slang hints. Rate-limited and service might be disrupted.");
                            ui.end_row();
                        }
                        Backend::Claude => {
                            ui.label("Claude API key");
                            ui.vertical(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.draft.claude_api_key).password(true));
                                if let Some(problem) = key_problem(&self.draft.claude_api_key) {
                                    ui.colored_label(Color32::from_rgb(255, 120, 90), problem);
                                }
                            });
                            ui.end_row();
                            ui.label("Claude model");
                            let chosen = self.draft.claude_preset().filter(|_| !self.custom_claude_model);
                            let shown = chosen.map_or(CUSTOM_LABEL, |index| CLAUDE_MODELS[index].0);
                            egui::ComboBox::from_id_salt("claude_model").selected_text(shown).show_ui(ui, |ui| {
                                for (index, (label, id)) in CLAUDE_MODELS.iter().enumerate() {
                                    if ui.selectable_label(chosen == Some(index), *label).clicked() {
                                        self.draft.claude_model = id.to_string();
                                        self.custom_claude_model = false;
                                    }
                                }
                                if ui.selectable_label(chosen.is_none(), CUSTOM_LABEL).clicked() {
                                    self.custom_claude_model = true;
                                }
                            });
                            ui.end_row();
                            if chosen.is_none() {
                                ui.label("Model ID");
                                ui.text_edit_singleline(&mut self.draft.claude_model);
                                ui.end_row();
                            }
                        }
                        Backend::OpenAiCompatible => {
                            ui.label("Service and model");
                            let chosen = self.draft.endpoint_preset().filter(|_| !self.custom_endpoint);
                            let shown = chosen.map_or(CUSTOM_LABEL, |index| ENDPOINT_PRESETS[index].label);
                            egui::ComboBox::from_id_salt("endpoint").selected_text(shown).show_ui(ui, |ui| {
                                for (index, preset) in ENDPOINT_PRESETS.iter().enumerate() {
                                    if ui.selectable_label(chosen == Some(index), preset.label).clicked() {
                                        self.draft.api_base = preset.api_base.to_string();
                                        self.draft.model = preset.model.to_string();
                                        self.custom_endpoint = false;
                                    }
                                }
                                if ui.selectable_label(chosen.is_none(), CUSTOM_LABEL).clicked() {
                                    self.custom_endpoint = true;
                                }
                            });
                            ui.end_row();
                            if chosen.is_none() {
                                ui.label("API base URL");
                                ui.text_edit_singleline(&mut self.draft.api_base);
                                ui.end_row();
                                ui.label("Model");
                                ui.text_edit_singleline(&mut self.draft.model);
                                ui.end_row();
                            }
                            ui.label("API key");
                            ui.add(egui::TextEdit::singleline(&mut self.draft.api_key).password(true).hint_text("empty for Ollama"));
                            ui.end_row();
                        }
                    }
                    ui.label("Anchor to chat tab");
                    ui.vertical(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.draft.anchor_tab).hint_text("tab name, or empty"));
                        if !self.draft.anchor_tab.trim().is_empty() {
                            ui.label(self.anchor_status());
                        }
                    });
                    ui.end_row();
                    ui.label("Reply in");
                    egui::ComboBox::from_id_salt("reply_variant")
                        .selected_text(self.draft.reply_variant.label())
                        .show_ui(ui, |ui| {
                            for variant in [ReplyVariant::Taiwan, ReplyVariant::Simplified] {
                                ui.selectable_value(&mut self.draft.reply_variant, variant, variant.label());
                            }
                        });
                    ui.end_row();
                    // On a chat tab the overlay takes the tab's place, size, text size and
                    // background, so its own are only offered while it is placed by hand.
                    let on_tab = layout::anchor(&self.chat_windows, &self.draft.anchor_tab).is_some();
                    let waiting = !self.draft.anchor_tab.trim().is_empty() && self.chat_windows.is_empty();
                    if !on_tab && !waiting {
                        ui.label("Overlay width");
                        ui.add(egui::Slider::new(&mut self.draft.overlay_width, 200.0..=1600.0));
                        ui.end_row();
                        ui.label("Overlay height");
                        ui.add(egui::Slider::new(&mut self.draft.overlay_height, 80.0..=1000.0));
                        ui.end_row();
                        ui.label("Background opacity");
                        ui.add(egui::Slider::new(&mut self.draft.overlay_opacity, 0.0..=1.0));
                        ui.end_row();
                        ui.label("Text size");
                        ui.add(egui::Slider::new(&mut self.draft.font_size, 10.0..=32.0));
                        ui.end_row();
                    }
                    ui.label("Lines kept (scrollback)");
                    ui.add(egui::Slider::new(&mut self.draft.max_lines, 1..=500));
                    ui.end_row();
                });
                if ui.button("Save settings").clicked() {
                    self.save();
                }
            });
        }
    }

    /// One line for the settings: what the anchor name currently matches.
    fn anchor_status(&self) -> String {
        match layout::anchor(&self.chat_windows, &self.draft.anchor_tab) {
            Some(window) if window.shown => format!("On the \"{}\" tab.", window.name),
            Some(window) => format!("Hidden until the \"{}\" tab is selected.", window.name),
            None if self.chat_windows.is_empty() => "Waiting for the addon to report the chat windows.".into(),
            None => {
                let names: Vec<&str> = self.chat_windows.iter().map(|window| window.name.as_str()).collect();
                format!("No such tab. The game has: {}", names.join(", "))
            }
        }
    }

    fn overlay(&mut self, ctx: &egui::Context) {
        // On a chat tab the overlay takes that window's place, size and font size.
        let anchor = layout::anchor(&self.chat_windows, &self.draft.anchor_tab).cloned();
        let size = match &anchor {
            Some(window) => [window.rect[2].max(40.0), window.rect[3].max(20.0)],
            None => [self.draft.overlay_width, self.draft.overlay_height],
        };
        let builder = ViewportBuilder::default()
            .with_title("Translate overlay")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_mouse_passthrough(self.locked)
            .with_position(self.overlay_start)
            .with_inner_size(size);

        let locked = self.locked;
        // Hiding parks the window off screen instead of closing or hiding it:
        // showing a window again can take focus from WoW, and Windows still
        // draws a border around an empty transparent window.
        let area = match &anchor {
            Some(window) => Some(window.rect),
            None => self.overlay_rect.map(|r| [r.min.x, r.min.y, r.width(), r.height()]),
        };
        let covered = locked && self.draft.hide_when_unfocused && !self.wow_view.shows_overlay(area);
        let hidden = covered || anchor.as_ref().is_some_and(|window| !window.shown);
        let pos = match &anchor {
            _ if hidden => egui::pos2(OFF_SCREEN, OFF_SCREEN),
            Some(window) => egui::pos2(window.rect[0], window.rect[1]),
            None => self.overlay_rect.map_or(self.overlay_start, |r| r.min),
        };
        // Placed by hand, the window is only moved to park and unpark it, so dragging it
        // is not fought. On a chat tab it follows the tab.
        let changed_mode = hidden != self.overlay_hidden || anchor.is_some() != self.overlay_anchored;
        if changed_mode || (anchor.is_some() && self.overlay_sent != Some(pos)) {
            ctx.send_viewport_cmd_to(overlay_id(), ViewportCommand::OuterPosition(pos));
            self.overlay_sent = Some(pos);
            self.overlay_hidden = hidden;
            self.overlay_anchored = anchor.is_some();
        }
        let mut draft = self.draft.clone();
        if let Some(window) = &anchor {
            draft.font_size = window.font_size.clamp(6.0, 48.0);
            // The chat window behind the overlay already draws its own background.
            draft.overlay_opacity = 0.0;
        }
        let anchored = anchor.is_some();
        let lines = &self.lines;
        let mut rect = self.overlay_rect;
        let wheel_steps = std::mem::take(&mut self.wheel_steps);

        ctx.show_viewport_immediate(overlay_id(), builder, |root, _class| {
            let ctx = root.ctx().clone();
            if hidden {
                return;
            }
            // Skip the parked position, which can linger for a frame after showing.
            if !anchored {
                rect = ctx.input(|i| i.viewport().outer_rect).filter(|r| r.min.x > OFF_SCREEN / 2.0).or(rect);
            }
            let alpha = (draft.overlay_opacity * 255.0) as u8;
            let fill = if locked {
                Color32::from_black_alpha(alpha)
            } else {
                Color32::from_rgba_unmultiplied(20, 60, 110, alpha.max(140))
            };
            let frame = egui::Frame::NONE.fill(fill).inner_margin(8.0);
            egui::CentralPanel::default().frame(frame).show(root, |ui| {
                if !locked && anchored {
                    ui.label(RichText::new("Following the chat tab. Lock it from the companion window.").color(Color32::WHITE));
                } else if !locked {
                    let drag = ui.interact(ui.max_rect(), ui.id().with("drag"), egui::Sense::drag());
                    if drag.drag_started() {
                        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                    }
                    ui.label(RichText::new("Drag to move. Lock it from the companion window.").color(Color32::WHITE));
                }
                let room = ui.available_height();
                egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink(false).show(ui, |ui| {
                    // Chat lines sit tight together and fill the window from the bottom up.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let font = egui::FontId::new(draft.font_size, egui::FontFamily::Name(crate::OVERLAY_FAMILY.into()));
                    let small = egui::FontId::new(draft.font_size * 0.85, font.family.clone());
                    let width = ui.available_width() - SHADOW;
                    if wheel_steps != 0.0 {
                        // Wheel up moves the lines down, bringing earlier ones into view.
                        let line_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
                        ui.scroll_with_delta(egui::vec2(0.0, wheel_steps * WHEEL_LINES * line_height));
                    }
                    let mut galleys = Vec::new();
                    for line in lines {
                        galleys.push(ui.painter().layout_job(line.job(&font, width)));
                        if draft.show_original && line.translated.is_some() {
                            let original = markup::plain(&line.line.body);
                            let format = TextFormat::simple(small.clone(), Color32::from_gray(170));
                            let mut job = LayoutJob::single_section(original, format);
                            job.wrap.max_width = width;
                            galleys.push(ui.painter().layout_job(job));
                        }
                    }
                    let height: f32 = galleys.iter().map(|galley| galley.size().y).sum();
                    ui.add_space((room - height - SHADOW).max(0.0));
                    for galley in galleys {
                        let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
                        // The game draws chat with a dark shadow one step down and right.
                        let shadow = rect.min + egui::vec2(SHADOW, SHADOW);
                        ui.painter().galley_with_override_text_color(shadow, galley.clone(), Color32::BLACK);
                        ui.painter().galley(rect.min, galley, Color32::WHITE);
                    }
                });
            });
        });
        self.overlay_rect = rect;
    }
}

impl eframe::App for TranslateApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain_events();
        self.control_panel(ui);
        self.overlay(&ctx);
        // After both windows laid out their text, so this frame's new glyphs are included.
        self.refresh_font_texture(&ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The overlay scrolls by calling `scroll_with_delta` inside its scroll area; this
    /// checks that doing so moves a scroll area that sticks to the bottom, and which way.
    #[test]
    fn wheel_steps_scroll_back_through_earlier_lines() {
        let ctx = egui::Context::default();
        let offset = |steps: f32| {
            let mut offset = 0.0;
            let mut output = ctx.run_ui(Default::default(), |ui| {
                ui.set_max_height(100.0);
                let area = egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink(false).animated(false);
                let shown = area.show(ui, |ui| {
                    if steps != 0.0 {
                        ui.scroll_with_delta(egui::vec2(0.0, steps * 30.0));
                    }
                    ui.allocate_exact_size(egui::vec2(50.0, 1000.0), egui::Sense::hover());
                });
                offset = shown.state.offset.y;
            });
            output.textures_delta.clear();
            offset
        };
        offset(0.0);
        let bottom = offset(0.0);
        assert!(bottom > 800.0, "starts at the bottom: {bottom}");
        let up = offset(2.0);
        assert!((bottom - up - 60.0).abs() < 1.0, "two steps up show earlier lines: {bottom} -> {up}");
        assert!((offset(0.0) - up).abs() < 1.0, "and it stays there");
        assert!((offset(-5.0) - bottom).abs() < 1.0, "scrolling down stops at the bottom");
    }
}
