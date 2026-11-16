use crate::backends::claude::key_problem;
use crate::config::{self, Backend, Config, ReplyVariant};
use crate::translate::WOW_CHAT_BYTES;
use crate::focus::WowView;
use crate::layout::{self, ChatWindow};
use crate::worker::{Job, UiEvent};
use eframe::egui::{self, Color32, RichText, ViewportBuilder, ViewportCommand, ViewportId};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
const HIDE_LABEL: &str = "Hide when WoW is hidden or covered";
#[cfg(not(target_os = "macos"))]
const HIDE_LABEL: &str = "Hide when WoW is in the background";

/// How often the whole font texture is re-sent even without new text.
const FONT_TEXTURE_REFRESH: Duration = Duration::from_secs(5);

struct OverlayLine {
    /// The number `UiEvent::Translated` refers to; 0 for lines that never get one.
    id: u64,
    speaker: String,
    original: String,
    /// None while a Chinese line waits for its translation.
    english: Option<String>,
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
            draft,
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
            test_result: String::new(),
            strip_status: "Starting the pixel strip reader".into(),
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
                UiEvent::Line { id, speaker, original, english } => {
                    self.push_line(OverlayLine { id, speaker, original, english });
                }
                UiEvent::Translated { id, english } => {
                    if let Some(line) = self.lines.iter_mut().rev().find(|line| line.id == id) {
                        line.english = Some(english);
                        self.font_texture_stale = true;
                    }
                }
                UiEvent::PasteReady { id, english } => {
                    self.notice = format!("#{id} ready: press Ctrl+V in the addon window");
                    self.push_line(OverlayLine {
                        id: 0,
                        speaker: format!("#{id}"),
                        original: String::new(),
                        english: Some(english),
                    });
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
                            ui.label("No setup. Chat lines are sent to Google. No slang hints;\nGoogle may slow down or stop this free service.");
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
                            ui.text_edit_singleline(&mut self.draft.claude_model);
                            ui.end_row();
                        }
                        Backend::OpenAiCompatible => {
                            ui.label("API base URL");
                            ui.text_edit_singleline(&mut self.draft.api_base);
                            ui.end_row();
                            ui.label("API key");
                            ui.add(egui::TextEdit::singleline(&mut self.draft.api_key).password(true));
                            ui.end_row();
                            ui.label("Model");
                            ui.text_edit_singleline(&mut self.draft.model);
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
                    ui.label("Overlay width");
                    ui.add(egui::Slider::new(&mut self.draft.overlay_width, 200.0..=1600.0));
                    ui.end_row();
                    ui.label("Overlay height");
                    ui.add(egui::Slider::new(&mut self.draft.overlay_height, 80.0..=1000.0));
                    ui.end_row();
                    ui.label("Background opacity").on_hover_text("Not used on a chat tab, which has its own background.");
                    ui.add(egui::Slider::new(&mut self.draft.overlay_opacity, 0.0..=1.0));
                    ui.end_row();
                    ui.label("Text size");
                    ui.add(egui::Slider::new(&mut self.draft.font_size, 10.0..=32.0));
                    ui.end_row();
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
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    let family = egui::FontFamily::Name(crate::OVERLAY_FAMILY.into());
                    for line in lines {
                        // A Chinese line shows as it is, dimmed, until its translation arrives.
                        let (shown, color) = match &line.english {
                            Some(english) => (english, Color32::WHITE),
                            None => (&line.original, Color32::from_gray(190)),
                        };
                        let mut text =
                            RichText::new(shown).family(family.clone()).size(draft.font_size).color(color);
                        if line.speaker.is_empty() {
                            text = text.italics();
                        }
                        ui.horizontal_wrapped(|ui| {
                            if !line.speaker.is_empty() {
                                ui.label(
                                    RichText::new(format!("{}:", line.speaker))
                                        .family(family.clone())
                                        .size(draft.font_size)
                                        .color(Color32::from_rgb(255, 210, 120)),
                                );
                            }
                            ui.label(text);
                        });
                        if draft.show_original && line.english.is_some() && !line.original.is_empty() {
                            ui.label(
                                RichText::new(&line.original)
                                    .family(family.clone())
                                    .size(draft.font_size * 0.85)
                                    .color(Color32::from_gray(170)),
                            );
                        }
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
