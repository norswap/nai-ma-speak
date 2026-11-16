use crate::backends::claude::key_problem;
use crate::config::{self, Backend, Config, ReplyVariant, CLAUDE_MODELS, CUSTOM_LABEL, ENDPOINT_PRESETS};
use crate::translate::WOW_CHAT_BYTES;
use crate::focus::WowView;
use crate::layout::{self, ChatWindow, Command};
use crate::markup::{self, ChatLine, Span};
use crate::theme;
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
    tab: Tab,
    /// Where the last reply stands; shown as a pill on the overlay.
    reply_state: ReplyState,
    /// The reply box is open, and whether it still has to take the keyboard.
    reply_box: bool,
    reply_box_focus: bool,
    /// The game's Settings button asked for this window to come forward.
    show_window: bool,
    /// This window was hidden with its Hide button.
    window_hidden: bool,
    /// The game's Settings button brought this window over the game, where it still is.
    window_summoned: bool,
    /// Whether the companion was the app in front at the last look.
    was_active: bool,
}

/// Where a reply typed in English stands.
enum ReplyState {
    Idle,
    Translating,
    /// Copied for pasting; goes back to idle a little later.
    Copied(Instant),
    Failed(String),
    /// Translated, but longer than one chat message, so not copied.
    TooLong,
}

/// How long the overlay says a reply was copied.
const COPIED_SHOWN: Duration = Duration::from_secs(6);
/// Size of the reply box, and how far above the chat window it opens, past the tab row.
const REPLY_BOX_HEIGHT: f32 = 86.0;
const REPLY_BOX_GAP: f32 = 34.0;

fn reply_id() -> ViewportId {
    ViewportId::from_hash_of("reply")
}

/// The sections of the companion window.
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Reply,
    Translation,
    Overlay,
    Status,
    History,
}

impl Tab {
    /// Typing a reply here comes last: in the game its own Translate button does that.
    const ALL: [(Tab, &'static str); 5] = [
        (Tab::Translation, "Backend"),
        (Tab::Overlay, "Overlay"),
        (Tab::Status, "Status"),
        (Tab::History, "Chat history"),
        (Tab::Reply, "Translate"),
    ];
}

/// Smallest companion window that shows every control with Settings closed.
pub const MIN_WIDTH: f32 = 560.0;
pub const MIN_HEIGHT: f32 = 150.0;

const OFF_SCREEN: f32 = -32000.0;
/// Whether a hidden overlay is moved off screen; see `overlay`.
const PARK_HIDDEN_OVERLAY: bool = !cfg!(target_os = "macos");

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
            tab: Tab::Translation,
            reply_state: ReplyState::Idle,
            reply_box: false,
            reply_box_focus: false,
            show_window: false,
            window_hidden: false,
            window_summoned: false,
            was_active: true,
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
                    self.reply_state = if copied { ReplyState::Copied(Instant::now()) } else { ReplyState::TooLong };
                    self.last_reply = Some((chinese, copied));
                }
                UiEvent::ReplyFailed { english, reason } => {
                    // The English goes back into the box, for another try when it is next opened.
                    self.reply = english;
                    self.reply_state = ReplyState::Failed(reason);
                }
                UiEvent::Command(Command::OpenReply) => {
                    // The game's button opens the box, and closes it when it is open.
                    self.reply_box = !self.reply_box;
                    self.reply_box_focus = self.reply_box;
                }
                UiEvent::Command(Command::OpenSettings) => self.show_window = true,
                UiEvent::Command(Command::Reply(english)) => self.send_reply(english),
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

    fn send_reply(&mut self, english: String) {
        let _ = self.interactive.send(Job::Reply { text: english });
        self.reply_state = ReplyState::Translating;
        self.last_reply = None;
    }

    /// The small window for typing a reply, opened by the game's Translate button above the
    /// chat window. Enter sends the text off and closes it at once, so it never sits over
    /// the game while the translation is on its way; the overlay's pill shows that.
    fn reply_box(&mut self, ctx: &egui::Context) {
        if !self.reply_box {
            return;
        }
        let window = layout::anchor(&self.chat_windows, &self.draft.anchor_tab).map(|window| window.rect);
        let area = window.or(self.overlay_rect.map(|r| [r.min.x, r.min.y, r.width(), r.height()]));
        let width = area.map_or(520.0, |area| area[2].clamp(380.0, 720.0));
        let mut builder = ViewportBuilder::default()
            .with_title("Translate reply")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_active(true)
            .with_inner_size([width, REPLY_BOX_HEIGHT]);
        if let Some(area) = area {
            builder = builder.with_position([area[0], area[1] - REPLY_BOX_HEIGHT - REPLY_BOX_GAP]);
        }
        let (note, edge) = match &self.reply_state {
            ReplyState::Failed(reason) => (format!("Not translated: {reason} Enter tries again."), theme::BAD),
            ReplyState::TooLong => {
                let bytes = self.last_reply.as_ref().map_or(0, |(chinese, _)| chinese.len());
                (format!("Not copied: {bytes} of {WOW_CHAT_BYTES} bytes. Say it in fewer words."), theme::GOLD)
            }
            _ => ("Enter translates and copies, and this box closes. Esc closes.".to_string(), theme::BORDER),
        };
        let too_long = matches!(self.reply_state, ReplyState::TooLong);
        let take_focus = std::mem::take(&mut self.reply_box_focus);
        let (mut submit, mut close, mut copy_anyway) = (false, false, false);
        let reply = &mut self.reply;
        ctx.show_viewport_immediate(reply_id(), builder, |root, _class| {
            let ctx = root.ctx().clone();
            if take_focus {
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
            let frame = egui::Frame::NONE
                .fill(theme::BACKGROUND)
                .stroke(egui::Stroke::new(2.0, edge))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 10));
            egui::CentralPanel::default().frame(frame).show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Translate").font(theme::heading_font(16.0)).color(theme::GOLD));
                    let edit = ui.add(theme::field(reply));
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = !reply.trim().is_empty();
                    } else {
                        // The box is only ever open to be typed in.
                        edit.request_focus();
                    }
                });
                ui.horizontal(|ui| {
                    if too_long {
                        copy_anyway = ui.add(theme::button("Copy anyway").small()).clicked();
                    }
                    ui.add(egui::Label::new(RichText::new(&note).size(11.5).color(if edge == theme::BORDER { theme::MUTED } else { edge })).truncate());
                });
                close = ui.input(|i| i.key_pressed(egui::Key::Escape) || i.viewport().close_requested());
            });
        });
        if copy_anyway {
            if let Some((chinese, copied)) = &mut self.last_reply {
                ctx.copy_text(chinese.clone());
                *copied = true;
                self.reply_state = ReplyState::Copied(Instant::now());
            }
        }
        if submit {
            let english = std::mem::take(&mut self.reply).trim().to_string();
            self.send_reply(english);
        }
        if submit || close || copy_anyway {
            self.reply_box = false;
            crate::focus::activate_wow();
        }
    }

    /// What the overlay's pill says about the last reply: its text, color and whether it
    /// is still under way. Nothing while there is nothing to say.
    fn reply_pill(&mut self, ctx: &egui::Context) -> Option<(String, Color32, bool)> {
        if let ReplyState::Copied(since) = self.reply_state {
            match COPIED_SHOWN.checked_sub(since.elapsed()) {
                Some(left) => ctx.request_repaint_after(left),
                None => self.reply_state = ReplyState::Idle,
            }
        }
        match &self.reply_state {
            ReplyState::Idle => None,
            ReplyState::Translating => Some(("Translating".into(), theme::TEXT, true)),
            ReplyState::Copied(_) => Some(("Copied, paste it".into(), theme::GOOD, false)),
            ReplyState::Failed(_) => Some(("Not translated: press Translate".into(), theme::BAD, false)),
            ReplyState::TooLong => Some(("Too long: press Translate".into(), theme::GOLD, false)),
        }
    }

    /// Brings this window over the game, next to the chat window, when the game's
    /// Settings button asks for it.
    /// Hides this window and gives the keyboard back to the game. The overlay stays.
    fn hide_window(&mut self, ctx: &egui::Context) {
        let root = ViewportId::ROOT;
        self.window_hidden = true;
        self.window_summoned = false;
        ctx.send_viewport_cmd_to(root, ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
        ctx.send_viewport_cmd_to(root, ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd_to(root, ViewportCommand::Visible(false));
        crate::focus::activate_wow();
    }

    /// Shows, hides and brings back this window: over the game, next to the chat window,
    /// when the game's Settings button asks for it, and away again on a second click.
    fn show_over_game(&mut self, ctx: &egui::Context) {
        let root = ViewportId::ROOT;
        // The yellow button would minimize the window, and macOS then ignores a click
        // on the app's icon as long as the companion is the app in front. Hiding the
        // window instead behaves the same as the Hide button, which comes back.
        if ctx.input(|i| i.viewport().minimized) == Some(true) {
            self.hide_window(ctx);
        }
        // A click on the app's icon makes the companion the app in front, but macOS
        // leaves its window hidden, because the overlay counts as a window already
        // showing. So the window is brought back here. Opening the reply box also puts
        // the companion in front, and must not bring it back.
        let active = crate::focus::app_is_active().unwrap_or(false);
        let activated = active && !self.was_active && !self.reply_box;
        self.was_active = active;
        if self.window_hidden {
            // Nothing else may ask for a pass while the app waits to be clicked.
            ctx.request_repaint_after(Duration::from_millis(300));
        }
        let asked = std::mem::take(&mut self.show_window);
        if asked && !self.window_hidden && (self.window_summoned || active) {
            self.hide_window(ctx);
            return;
        }
        if !asked && !(self.window_hidden && activated) {
            return;
        }
        self.window_hidden = false;
        ctx.send_viewport_cmd_to(root, ViewportCommand::Visible(true));
        ctx.send_viewport_cmd_to(root, ViewportCommand::Minimized(false));
        if asked {
            // From the game: stay above it, next to the chat window.
            self.window_summoned = true;
            ctx.send_viewport_cmd_to(root, ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
            if let Some(window) = layout::anchor(&self.chat_windows, &self.draft.anchor_tab) {
                let [x, y, width, height] = window.rect;
                let top = (y + height - self.fitted_height).max(y - 300.0);
                ctx.send_viewport_cmd_to(root, ViewportCommand::OuterPosition(egui::pos2(x + width + 24.0, top)));
            }
        }
        ctx.send_viewport_cmd_to(root, ViewportCommand::Focus);
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
        let frame = egui::Frame::NONE.fill(theme::BACKGROUND);
        let panel = egui::CentralPanel::default().frame(frame).show(root, |ui| {
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
        let margin = 6.0;
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
        // The title bar, the tabs and the page touch, with no gap showing between them.
        let spacing = std::mem::replace(&mut ui.spacing_mut().item_spacing.y, 0.0);
        self.title_bar(ui);
        self.tab_bar(ui);
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(18, 14)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = spacing;
            // Text takes the window's width and wraps; nothing is cut off at the edge.
            ui.set_width(ui.available_width());
            if !self.notice.is_empty() {
                ui.label(RichText::new(&self.notice).color(theme::MUTED));
                ui.add_space(4.0);
            }
            match self.tab {
                Tab::Reply => self.reply_tab(ui),
                Tab::Translation => self.translation_tab(ui),
                Tab::Overlay => self.overlay_tab(ui),
                Tab::Status => self.status_tab(ui),
                Tab::History => self.history_tab(ui),
            }
        });
    }

    /// The name, whether translation is working and with what, and the window's own buttons.
    fn title_bar(&mut self, ui: &mut egui::Ui) {
        let bar = egui::Frame::NONE.fill(theme::BAR).inner_margin(egui::Margin::symmetric(14, 8));
        bar.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("WoW Translate").font(theme::heading_font(21.0)).color(theme::GOLD));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let hint = "Hide this window; the overlay stays. The game's Settings button or the app's icon brings it back.";
                    if ui.add(theme::button("Hide")).on_hover_text(hint).clicked() {
                        self.hide_window(ui.ctx());
                    }
                    if ui.add(theme::button("Restart")).on_hover_text("Quit and start the companion again. Unsaved settings are lost.").clicked() {
                        match config::start_again() {
                            Ok(()) => ui.ctx().send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close),
                            Err(e) => self.notice = format!("Could not restart: {e}"),
                        }
                    }
                    // What is left of the row, so a long status is shortened, not pushed out.
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let working = !self.status.starts_with("Translation failed");
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 4.0, if working { theme::GOOD } else { theme::BAD });
                        // Chat lines leave this computer for the service named here, unless it runs locally.
                        let status = format!("{} · {}", self.status, self.draft.backend.short_label());
                        ui.add(egui::Label::new(status).truncate());
                    });
                });
            });
        });
    }

    /// The tabs stand on a line; the chosen one is open at the bottom, joined to its page.
    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let bar = egui::Frame::NONE.fill(theme::BAR).inner_margin(egui::Margin { left: 14, right: 14, top: 4, bottom: 0 });
        let mut chosen = egui::Rect::NOTHING;
        let row = bar.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (tab, name) in Tab::ALL {
                    let selected = self.tab == tab;
                    let color = if selected { Color32::WHITE } else { theme::GOLD };
                    let text = ui.painter().layout_no_wrap(name.to_string(), theme::heading_font(15.0), color);
                    let (rect, response) = ui.allocate_exact_size(text.size() + egui::vec2(28.0, 14.0), egui::Sense::click());
                    // The other tabs are a little lower and darker.
                    let shape = if selected { rect } else { rect.with_min_y(rect.top() + 3.0) };
                    let (fill, edge) = match (selected, response.hovered()) {
                        (true, _) => (theme::BACKGROUND, theme::BORDER),
                        (false, true) => (theme::BACKGROUND, theme::BORDER_FIELD),
                        (false, false) => (theme::FIELD, theme::BORDER_DIM),
                    };
                    let corners = egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 };
                    ui.painter().rect(shape, corners, fill, egui::Stroke::new(1.0, edge), egui::StrokeKind::Inside);
                    ui.painter().galley(shape.center() - text.size() / 2.0, text, color);
                    if selected {
                        chosen = rect;
                    }
                    if response.clicked() {
                        self.tab = tab;
                    }
                }
                ui.add_space(ui.available_width());
            });
        });
        let bottom = row.response.rect.bottom();
        let painter = ui.painter();
        painter.hline(row.response.rect.x_range(), bottom - 0.5, egui::Stroke::new(1.0, theme::BORDER));
        // Rub the line out under the chosen tab, between its side edges.
        let gap = (chosen.left() + 1.0)..=(chosen.right() - 1.0);
        painter.hline(gap, bottom - 0.5, egui::Stroke::new(1.0, theme::BACKGROUND));
    }

    fn save_row(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(theme::main_button("Save settings")).clicked() {
                self.save();
            }
            ui.label(RichText::new("Changes apply when saved.").color(theme::MUTED));
        });
    }

    fn reply_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::label("Translate into Chinese"));
        let edit = ui.add(
            theme::field(&mut self.reply).hint_text("Type in English, press Enter, then paste into WoW chat"),
        );
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.reply.trim().is_empty() {
            let english = std::mem::take(&mut self.reply).trim().to_string();
            self.send_reply(english);
            self.notice = "Translating your reply".into();
            edit.request_focus();
        }
        // WoW's chat box holds 255 bytes, and a Chinese character takes 3 of them.
        let mut copy_anyway = false;
        if let Some((chinese, copied)) = &self.last_reply {
            let size = format!("{}/{WOW_CHAT_BYTES} bytes", chinese.len());
            if *copied {
                ui.label(format!("{size}: {chinese}"));
            } else {
                ui.label(RichText::new(format!("{size}, WoW would cut it off: {chinese}")).color(theme::BAD));
                copy_anyway = ui.add(theme::button("Copy anyway")).clicked();
            }
        }
        if copy_anyway {
            if let Some((chinese, copied)) = &mut self.last_reply {
                ui.ctx().copy_text(chinese.clone());
                *copied = true;
                self.notice = "Copied the long reply. WoW will cut it at 255 bytes.".into();
            }
        }
    }

    fn translation_tab(&mut self, ui: &mut egui::Ui) {
        let wide = ui.available_width();
        ui.label(theme::label("Translate with"));
        egui::ComboBox::from_id_salt("backend").width(wide).selected_text(self.draft.backend.label()).show_ui(ui, |ui| {
            for backend in Backend::ALL {
                ui.selectable_value(&mut self.draft.backend, backend, backend.label());
            }
        });
        match self.draft.backend {
            Backend::GoogleFree => {
                ui.label(RichText::new("No slang hints. Rate-limited and service might be disrupted.").color(theme::MUTED));
            }
            Backend::Claude => {
                ui.label(theme::label("Claude API key"));
                ui.add(theme::field(&mut self.draft.claude_api_key).password(true));
                if let Some(problem) = key_problem(&self.draft.claude_api_key) {
                    ui.label(RichText::new(problem).color(theme::BAD));
                }
                ui.label(theme::label("Claude model"));
                let chosen = self.draft.claude_preset().filter(|_| !self.custom_claude_model);
                let shown = chosen.map_or(CUSTOM_LABEL, |index| CLAUDE_MODELS[index].0);
                egui::ComboBox::from_id_salt("claude_model").width(wide).selected_text(shown).show_ui(ui, |ui| {
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
                if chosen.is_none() {
                    ui.label(theme::label("Model ID"));
                    ui.add(theme::field(&mut self.draft.claude_model));
                }
            }
            Backend::OpenAiCompatible => {
                ui.label(theme::label("Service and model"));
                let chosen = self.draft.endpoint_preset().filter(|_| !self.custom_endpoint);
                let shown = chosen.map_or(CUSTOM_LABEL, |index| ENDPOINT_PRESETS[index].label);
                egui::ComboBox::from_id_salt("endpoint").width(wide).selected_text(shown).show_ui(ui, |ui| {
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
                if chosen.is_none() {
                    ui.label(theme::label("API base URL"));
                    ui.add(theme::field(&mut self.draft.api_base));
                    ui.label(theme::label("Model"));
                    ui.add(theme::field(&mut self.draft.model));
                }
                ui.label(theme::label("API key"));
                ui.add(theme::field(&mut self.draft.api_key).password(true).hint_text("empty for Ollama"));
            }
        }
        ui.label(theme::label("Reply in"));
        egui::ComboBox::from_id_salt("reply_variant")
            .width(wide)
            .selected_text(self.draft.reply_variant.label())
            .show_ui(ui, |ui| {
                for variant in [ReplyVariant::Taiwan, ReplyVariant::Simplified] {
                    ui.selectable_value(&mut self.draft.reply_variant, variant, variant.label());
                }
            });

        ui.add_space(6.0);
        let boxed = egui::Frame::NONE
            .fill(theme::FIELD)
            .stroke(egui::Stroke::new(1.0, theme::BORDER_DIM))
            .corner_radius(4)
            .inner_margin(10);
        boxed.show(ui, |ui| {
            ui.set_width(ui.available_width());
            if ui.add(theme::button("Test translation")).clicked() {
                // Test what is chosen here, even before Save.
                *self.config.lock().unwrap() = self.draft.clone();
                let _ = self.interactive.send(Job::Test);
                self.test_result = format!("Testing {}", self.draft.backend.label());
            }
            if !self.test_result.is_empty() {
                ui.label(&self.test_result);
            }
        });
        self.save_row(ui);
    }

    fn overlay_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let label = if self.locked { "Unlock overlay to move it" } else { "Lock overlay" };
            if ui.add(theme::button(label)).clicked() {
                self.locked = !self.locked;
                if self.locked {
                    self.save();
                }
            }
            if ui.add(theme::button("Clear overlay")).clicked() {
                self.lines.clear();
            }
        });
        ui.checkbox(&mut self.draft.show_original, "Show Chinese too");
        ui.checkbox(&mut self.draft.hide_when_unfocused, HIDE_LABEL);

        ui.label(theme::label("Anchor to chat tab"));
        ui.add(theme::field(&mut self.draft.anchor_tab).hint_text("tab name, or empty"));
        if !self.draft.anchor_tab.trim().is_empty() {
            ui.label(RichText::new(self.anchor_status()).color(theme::MUTED));
        }
        // On a chat tab the overlay takes the tab's place, size, text size and
        // background, so its own are only offered while it is placed by hand.
        let on_tab = layout::anchor(&self.chat_windows, &self.draft.anchor_tab).is_some();
        let waiting = !self.draft.anchor_tab.trim().is_empty() && self.chat_windows.is_empty();
        ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(120.0);
        if !on_tab && !waiting {
            ui.label(theme::label("Overlay width"));
            ui.add(egui::Slider::new(&mut self.draft.overlay_width, 200.0..=1600.0));
            ui.label(theme::label("Overlay height"));
            ui.add(egui::Slider::new(&mut self.draft.overlay_height, 80.0..=1000.0));
            ui.label(theme::label("Background opacity"));
            ui.add(egui::Slider::new(&mut self.draft.overlay_opacity, 0.0..=1.0));
            ui.label(theme::label("Text size"));
            ui.add(egui::Slider::new(&mut self.draft.font_size, 10.0..=32.0));
        }
        ui.label(theme::label("Lines kept (scrollback)"));
        ui.add(egui::Slider::new(&mut self.draft.max_lines, 1..=500));
        self.save_row(ui);
    }

    /// The lines the overlay holds, to scroll through and select text from.
    fn history_tab(&mut self, ui: &mut egui::Ui) {
        let boxed = egui::Frame::NONE
            .fill(theme::FIELD)
            .stroke(egui::Stroke::new(1.0, theme::BORDER_DIM))
            .corner_radius(4)
            .inner_margin(8);
        boxed.show(ui, |ui| {
            let scroll = egui::ScrollArea::vertical().id_salt("history").max_height(320.0).stick_to_bottom(true);
            scroll.auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let font = egui::FontId::new(14.0, egui::FontFamily::Name(crate::OVERLAY_FAMILY.into()));
                let width = ui.available_width();
                if self.lines.is_empty() {
                    ui.label(RichText::new("No chat lines yet.").color(theme::MUTED));
                }
                for line in &self.lines {
                    ui.label(line.job(&font, width));
                }
            });
        });
        ui.label(RichText::new("Holds as many lines as \"Lines kept\" on the Overlay tab.").color(theme::MUTED));
    }

    fn status_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::label("Translation"));
        ui.label(format!("{} · translating with {}", self.status, self.draft.backend.short_label()));
        ui.label(theme::label("Pixel strip"));
        ui.label(&self.strip_status);
        ui.label(theme::label("Chat windows"));
        if self.chat_windows.is_empty() {
            ui.label("Waiting for the addon to report the chat windows.");
        } else {
            let names: Vec<&str> = self.chat_windows.iter().map(|window| window.name.as_str()).collect();
            ui.label(names.join(", "));
        }
        ui.label(theme::label("Settings file"));
        ui.label(config::path().display().to_string());
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
        let locked = self.locked;
        // A hidden overlay is not closed or hidden: showing a window again can take
        // focus from WoW. On macOS it stays where it is and draws nothing. It must stay
        // on screen there: the app only runs while one of its windows can be seen, so
        // with the overlay parked and this window behind the game, nothing would run
        // and the overlay would never come back. Elsewhere it is parked off screen,
        // because Windows draws a border around an empty transparent window.
        let area = match &anchor {
            Some(window) => Some(window.rect),
            None => self.overlay_rect.map(|r| [r.min.x, r.min.y, r.width(), r.height()]),
        };
        let covered = locked && self.draft.hide_when_unfocused && !self.wow_view.shows_overlay(area);
        let hidden = covered || anchor.as_ref().is_some_and(|window| !window.shown);
        let builder = ViewportBuilder::default()
            .with_title("Translate overlay")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_mouse_passthrough(self.locked || hidden)
            .with_position(self.overlay_start)
            .with_inner_size(size);

        let pos = match &anchor {
            _ if hidden && PARK_HIDDEN_OVERLAY => egui::pos2(OFF_SCREEN, OFF_SCREEN),
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
        let mut rect = self.overlay_rect;
        let wheel_steps = std::mem::take(&mut self.wheel_steps);
        let pill = self.reply_pill(ctx);
        let lines = &self.lines;

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
                if let Some((text, color, busy)) = &pill {
                    // In the top right corner, over the oldest lines.
                    let galley = ui.painter().layout_no_wrap(text.clone(), egui::FontId::proportional(12.0), *color);
                    let (pad, spinner) = (egui::vec2(8.0, 5.0), if *busy { 17.0 } else { 0.0 });
                    let size = galley.size() + 2.0 * pad + egui::vec2(spinner, 0.0);
                    let corner = ui.max_rect().right_top();
                    let pill = egui::Rect::from_min_size(egui::pos2(corner.x - size.x, corner.y), size);
                    let edge = egui::Stroke::new(1.0, *color);
                    ui.painter().rect(pill, 4, theme::BACKGROUND, edge, egui::StrokeKind::Inside);
                    if *busy {
                        let at = egui::Rect::from_center_size(egui::pos2(pill.left() + pad.x + 6.0, pill.center().y), egui::vec2(12.0, 12.0));
                        egui::Spinner::new().size(12.0).color(theme::GOLD).paint_at(ui, at);
                    }
                    ui.painter().galley(pill.min + pad + egui::vec2(spinner, 0.0), galley, *color);
                }
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
        self.show_over_game(&ctx);
        self.overlay(&ctx);
        self.reply_box(&ctx);
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
