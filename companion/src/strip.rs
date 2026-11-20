//! Reads the addon's pixel strip off the screen. The layout must match
//! addon/NaiMaSpeak/Strip.lua: a frame of 64 x 8 blocks, each one of 64 colors: 3 sync
//! blocks, then [seq][len hi][len lo][text...][fletcher16 hi][fletcher16 lo] packed
//! 3 bytes into 4 blocks. Under the frame, a ninth row shows the 64 colors in order.

use crate::markup::ChatLine;
use crate::protocol::{self, Message};
use crate::worker::{Job, UiEvent, UiSender};
use std::collections::BTreeMap;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};
use xcap::image::RgbaImage;
use xcap::Monitor;

pub const COLS: usize = 64;
/// Rows of the frame; the palette row comes after them.
pub const ROWS: usize = 8;
const SYNC: [u8; 3] = [51, 15, 60];

/// Where the strip sits in an image, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lock {
    pub x: u32,
    pub y: u32,
    pub block: u32,
}

#[derive(Debug, PartialEq)]
pub struct Frame {
    pub seq: u8,
    pub checksum: u16,
    pub text: String,
}

#[derive(Debug, PartialEq)]
pub enum Decoded {
    Frame(Frame),
    /// Sync blocks and palette present but the data did not check out: caught while the
    /// game was drawing a new frame, or colors too close together to tell apart.
    Corrupt,
    /// Sync blocks present but no palette under the frame: an addon from before the
    /// palette.
    NoPalette,
    /// No strip at this spot.
    Missing,
}

type Color = [f32; 3];

/// The pixel's channels, or None outside the image.
fn pixel(img: &RgbaImage, x: u32, y: u32) -> Option<Color> {
    if x >= img.width() || y >= img.height() {
        return None;
    }
    let p = img.get_pixel(x, y).0;
    Some([p[0] as f32, p[1] as f32, p[2] as f32])
}

/// Reads a sync color using only on/off per channel: the strip has to be found before
/// its palette can be read.
fn sync_symbol(img: &RgbaImage, x: u32, y: u32) -> Option<u8> {
    let [r, g, b] = pixel(img, x, y)?;
    let on = |v: f32| if v >= 128.0 { 3 } else { 0 };
    Some(on(r) * 16 + on(g) * 4 + on(b))
}

fn distance(a: Color, b: Color) -> f32 {
    (0..3).map(|channel| (a[channel] - b[channel]).powi(2)).sum()
}

/// How far a sync block may be from its color in the palette, per channel, before the
/// row under the frame is taken for something other than a palette.
const SAME_COLOR: f32 = 24.0;

/// Whether a row of blocks is the palette: the 64 colors in order, as far as on and off
/// tell, which is all that is known of a color before the palette is read. Scenery that
/// happens to show the three sync colors in a row does not pass for a strip this way.
fn is_palette(row: &[Color; COLS]) -> bool {
    row.iter().enumerate().all(|(color, seen)| {
        (0..3).all(|channel| match (color >> (4 - 2 * channel)) & 3 {
            0 => seen[channel] < 128.0,
            3 => seen[channel] >= 128.0,
            _ => true,
        })
    })
}

fn fletcher16(bytes: &[u8]) -> u16 {
    let (mut a, mut b) = (0u16, 0u16);
    for &byte in bytes {
        a = (a + byte as u16) % 255;
        b = (b + a) % 255;
    }
    b * 256 + a
}

/// Reads the frame at `lock`. A screen capture does not return the colors the addon drew:
/// the display's color profile mixes the channels (on a MacBook's own screen magenta,
/// 255 0 255, comes back as 234 51 247) and the game bends the brightness levels. So no
/// color is assumed: each block is the palette color it is nearest to, and the palette
/// went through whatever the frame went through.
pub fn decode(img: &RgbaImage, lock: Lock) -> Decoded {
    let block = |i: usize| {
        let x = lock.x + (i % COLS) as u32 * lock.block + lock.block / 2;
        let y = lock.y + (i / COLS) as u32 * lock.block + lock.block / 2;
        pixel(img, x, y)
    };
    for (i, expected) in SYNC.iter().enumerate() {
        let (x, y) = (lock.x + i as u32 * lock.block + lock.block / 2, lock.y + lock.block / 2);
        if sync_symbol(img, x, y) != Some(*expected) {
            return Decoded::Missing;
        }
    }
    let mut palette = [[0.0; 3]; COLS];
    for (color, seen) in palette.iter_mut().enumerate() {
        match block(COLS * ROWS + color) {
            Some(pixel) => *seen = pixel,
            None => return Decoded::Missing,
        }
    }
    let in_palette = |(i, color): (usize, &u8)| {
        let (sync, listed) = (block(i).expect("read above"), palette[*color as usize]);
        (0..3).all(|channel| (sync[channel] - listed[channel]).abs() <= SAME_COLOR)
    };
    if !is_palette(&palette) || !SYNC.iter().enumerate().all(in_palette) {
        return Decoded::NoPalette;
    }
    let nearest = |pixel: Color| {
        let closer = |a: &usize, b: &usize| distance(palette[*a], pixel).total_cmp(&distance(palette[*b], pixel));
        (0..COLS).min_by(closer).expect("the palette is not empty") as u32
    };
    let mut symbols = Vec::with_capacity(COLS * ROWS);
    for i in SYNC.len()..COLS * ROWS {
        match block(i) {
            Some(pixel) => symbols.push(nearest(pixel)),
            None => return Decoded::Missing,
        }
    }
    let mut bytes = Vec::with_capacity(symbols.len() / 4 * 3);
    for chunk in symbols.as_chunks::<4>().0 {
        let v = chunk[0] << 18 | chunk[1] << 12 | chunk[2] << 6 | chunk[3];
        bytes.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8]);
    }
    let len = (bytes[1] as usize) << 8 | bytes[2] as usize;
    if 3 + len + 2 > bytes.len() {
        return Decoded::Corrupt;
    }
    let checksum = (bytes[3 + len] as u16) << 8 | bytes[4 + len] as u16;
    if fletcher16(&bytes[..3 + len]) != checksum {
        return Decoded::Corrupt;
    }
    match String::from_utf8(bytes[3..3 + len].to_vec()) {
        Ok(text) => Decoded::Frame(Frame { seq: bytes[0], checksum, text }),
        Err(_) => Decoded::Corrupt,
    }
}

/// Scans a whole image for the sync blocks and returns where the strip is.
pub fn find(img: &RgbaImage) -> Option<Lock> {
    let is = |x: u32, y: u32, s: u8| sync_symbol(img, x, y) == Some(s);
    for y in 0..img.height() {
        let mut x = 0;
        while x < img.width() {
            if !is(x, y, SYNC[0]) {
                x += 1;
                continue;
            }
            let start = x;
            while is(x, y, SYNC[0]) {
                x += 1;
            }
            let block = x - start;
            let cyan = (x..).take_while(|&cx| is(cx, y, SYNC[1])).count() as u32;
            let yellow = (x + cyan..).take_while(|&cx| is(cx, y, SYNC[2])).count() as u32;
            if cyan == 0 || yellow == 0 {
                continue;
            }
            // The middle one of the three runs: an edge pixel blended with what is beside
            // the strip shortens the first, and a first data block that passes for yellow
            // lengthens the last.
            let mut runs = [block, cyan, yellow];
            runs.sort_unstable();
            let block = runs[1];
            let mut top = y;
            while top > 0 && is(start, top - 1, SYNC[0]) {
                top -= 1;
            }
            let lock = Lock { x: start, y: top, block: block.max(1) };
            match decode(img, lock) {
                Decoded::Frame(_) | Decoded::Corrupt => return Some(lock),
                // An addon from before the palette, to say so: it drew blocks of 4
                // pixels. Without a palette to tell a strip by, smaller sync colors are
                // taken for scenery, which shows them now and then.
                Decoded::NoPalette if lock.block >= 4 => return Some(lock),
                Decoded::NoPalette | Decoded::Missing => {}
            }
        }
    }
    None
}

/// The strip's width and height in pixels, palette row included.
fn size(lock: Lock) -> (u32, u32) {
    (COLS as u32 * lock.block, (ROWS as u32 + 1) * lock.block)
}

/// The capture rectangle, in the monitor's points, that holds a strip found
/// at `lock` in a full capture taken in pixels. macOS takes regions in points
/// but captures in pixels, so on a HiDPI display (scale 2) the pixel
/// coordinates must be halved; one point of margin absorbs the rounding.
fn region_in_points(lock: Lock, scale: f32, max_w: u32, max_h: u32) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = size(lock);
    let x0 = (lock.x as f32 / scale).floor() as u32;
    let y0 = (lock.y as f32 / scale).floor() as u32;
    let x1 = (((lock.x + w) as f32 / scale).ceil() as u32 + 1).min(max_w);
    let y1 = (((lock.y + h) as f32 / scale).ceil() as u32 + 1).min(max_h);
    (x1 > x0 && y1 > y0).then(|| (x0, y0, x1 - x0, y1 - y0))
}

/// Reads the strip's own rectangle once it has been found. Returns what was captured
/// too, for a picture of a strip that cannot be read.
fn read_locked(monitor: &Monitor, lock: Lock) -> (Decoded, Option<RgbaImage>) {
    let scale = if cfg!(target_os = "macos") {
        monitor.scale_factor().unwrap_or(1.0).max(1.0)
    } else {
        1.0
    };
    if scale == 1.0 {
        let (w, h) = size(lock);
        return match monitor.capture_region(lock.x, lock.y, w, h) {
            Ok(img) => (decode(&img, Lock { x: 0, y: 0, block: lock.block }), Some(img)),
            Err(_) => (Decoded::Missing, None),
        };
    }
    let max_w = monitor.width().unwrap_or(u32::MAX);
    let max_h = monitor.height().unwrap_or(u32::MAX);
    let Some((x, y, w, h)) = region_in_points(lock, scale, max_w, max_h) else {
        return (Decoded::Missing, None);
    };
    match monitor.capture_region(x, y, w, h) {
        // The capture comes back in pixels; finding the strip again inside it
        // is cheap and avoids redoing the rounding.
        Ok(img) => (find(&img).map_or(Decoded::Missing, |found| decode(&img, found)), Some(img)),
        Err(_) => (Decoded::Missing, None),
    }
}


/// Polls the screen: a full scan about once a second until the strip turns
/// up, then only the strip's own rectangle several times a second.
///
/// Set NAIMASPEAK_DUMP=1 to save each full scan as nai-ma-speak-capture-<n>.png
/// in the working directory and log what was captured, for `--decode`.
/// `jobs` is the queue for chat lines as they come, `asked` the one for what the player
/// asks for and waits on.
/// How long the strip may go unread before the Status tab says so. A frame caught while
/// the game is drawing the next one fails its check, and the read after it succeeds.
const UNREADABLE_AFTER: Duration = Duration::from_secs(2);

/// How many line numbers are remembered; the addon repeats far fewer lines than this.
const HAD_MOST: usize = 2000;

/// Sets the Status tab's line about the strip, unless it says that already.
/// What the Status tab says while no strip is on any screen.
pub const LOOKING: &str = "Looking for the pixel strip. In game, type /tr /test.";

fn tell(ui: &UiSender, told: &mut String, text: String) {
    if *told != text {
        crate::diag::log(format!("strip: {text}"));
        ui.send(UiEvent::Strip(text.clone()));
        *told = text;
    }
}

pub fn spawn(jobs: Sender<Job>, asked: Sender<Job>, ui: UiSender) {
    thread::spawn(move || {
        let mut locked: Option<(usize, Lock)> = None;
        let mut last_seen: Option<(u8, u16)> = None;
        let mut next_line: u64 = 1;
        // The chat lines already had, by their numbers, for dropping the addon's repeats:
        // those shown before a restart, then each one as it comes. A number that comes
        // again with another line is that line worded anew (the game has since named a
        // link in it), and goes on to take its place.
        let mut had: BTreeMap<u64, ChatLine> =
            crate::backlog::load().into_iter().filter_map(|kept| Some((kept.line.number?, kept.line))).collect();
        let mut last_good = Instant::now();
        let mut unread_since: Option<Instant> = None;
        let mut told = String::new();
        let mut said_capture_error = false;
        let dump = std::env::var_os("NAIMASPEAK_DUMP").is_some();
        let mut pictured = false;

        loop {
            let monitors = match Monitor::all() {
                Ok(m) => m,
                Err(e) => {
                    ui.send(UiEvent::Status(format!("Screen capture unavailable: {e}")));
                    thread::sleep(Duration::from_secs(5));
                    continue;
                }
            };

            let Some((index, lock)) = locked else {
                tell(&ui, &mut told, LOOKING.into());
                for (index, monitor) in monitors.iter().enumerate() {
                    let img = match monitor.capture_image() {
                        Ok(img) => img,
                        Err(e) => {
                            // Used to be skipped silently, which looks exactly
                            // like "no strip on screen".
                            if !said_capture_error {
                                tell(&ui, &mut told, format!("Screen {} capture failed: {e}", index + 1));
                                said_capture_error = true;
                            }
                            continue;
                        }
                    };
                    if dump {
                        let path = format!("nai-ma-speak-capture-{}.png", index + 1);
                        eprintln!(
                            "screen {}: {}x{} points, scale {}, captured {}x{} px, saved to {path}",
                            index + 1,
                            monitor.width().unwrap_or(0),
                            monitor.height().unwrap_or(0),
                            monitor.scale_factor().unwrap_or(0.0),
                            img.width(),
                            img.height()
                        );
                        if let Err(e) = img.save(&path) {
                            eprintln!("could not save {path}: {e}");
                        }
                    }
                    if let Some(lock) = find(&img) {
                        crate::diag::log(format!(
                            "strip: found on screen {} at {},{} with {} px blocks; the screen is {}x{} points at scale {}, captured as {}x{} px",
                            index + 1,
                            lock.x,
                            lock.y,
                            lock.block,
                            monitor.width().unwrap_or(0),
                            monitor.height().unwrap_or(0),
                            monitor.scale_factor().unwrap_or(0.0),
                            img.width(),
                            img.height()
                        ));
                        locked = Some((index, lock));
                        last_good = Instant::now();
                        unread_since = None;
                        break;
                    }
                }
                if locked.is_none() {
                    thread::sleep(Duration::from_millis(1000));
                }
                continue;
            };

            let Some(monitor) = monitors.get(index) else {
                locked = None;
                continue;
            };
            let place = format!("on screen {} at {},{}", index + 1, lock.x, lock.y);
            // Finding the strip takes its three sync blocks; reading it takes every block.
            let (decoded, seen) = read_locked(monitor, lock);
            // Once the strip has stayed unreadable a while, what was captured at its place
            // is kept for the report, once.
            let mut unreadable = |why: &str| {
                let since = *unread_since.get_or_insert_with(Instant::now);
                if since.elapsed() <= UNREADABLE_AFTER {
                    return;
                }
                tell(&ui, &mut told, format!("Found the pixel strip {place}, but cannot read it: {why}"));
                if let (false, Some(seen)) = (pictured, &seen) {
                    pictured = true;
                    crate::diag::keep_strip(seen);
                }
            };
            match decoded {
                Decoded::Frame(frame) => {
                    last_good = Instant::now();
                    unread_since = None;
                    tell(&ui, &mut told, format!("Reading the pixel strip {place}"));
                    if last_seen != Some((frame.seq, frame.checksum)) {
                        // The first frame after locking may be an old line; it is
                        // still new to the overlay, so it is shown too.
                        let first = last_seen.is_none();
                        last_seen = Some((frame.seq, frame.checksum));
                        for message in frame.text.split('\n').filter_map(protocol::parse) {
                            match message {
                                Message::Windows(windows) => {
                                    let scale = if cfg!(target_os = "macos") {
                                        monitor.scale_factor().unwrap_or(1.0).max(1.0)
                                    } else {
                                        1.0
                                    };
                                    let picture = crate::layout::Picture {
                                        monitor: (monitor.x().unwrap_or(0) as f32, monitor.y().unwrap_or(0) as f32),
                                        scale,
                                        strip: (lock.x as f32, lock.y as f32),
                                        block: lock.block as f32,
                                    };
                                    let windows = windows.iter().map(|window| picture.on_screen(window)).collect();
                                    ui.send(UiEvent::ChatWindows(windows));
                                }
                                // A command still on the strip when the companion starts was
                                // given before it was watching, perhaps long ago.
                                Message::Command(_) if first => {}
                                Message::Command(command) => ui.send(UiEvent::Command(command)),
                                Message::Wheel { name, count } => ui.send(UiEvent::Wheel { name, count }),
                                Message::Version { addon, character } => {
                                    ui.send(UiEvent::AddonVersion(addon));
                                    ui.send(UiEvent::Character(character));
                                }
                                Message::Chat { line, handling } => {
                                    if let Some(number) = line.number {
                                        if had.get(&number) == Some(&line) {
                                            continue;
                                        }
                                        had.insert(number, line.clone());
                                    }
                                    while had.len() > HAD_MOST {
                                        had.pop_first();
                                    }
                                    let (event, job) = crate::worker::chat_line(next_line, line, handling);
                                    next_line += 1;
                                    ui.send(event);
                                    // A line the player asked for does not wait behind the chat.
                                    let queue = if handling.asked() { &asked } else { &jobs };
                                    if let Some(job) = job {
                                        let _ = queue.send(job);
                                    }
                                }
                            }
                        }
                    }
                }
                Decoded::Corrupt => unreadable("its frames fail their check. Run the companion with --decode on a screenshot to see more."),
                Decoded::NoPalette => unreadable("it has no palette row. Update the addon to the companion's version, then /reload."),
                Decoded::Missing => {
                    // Hidden UI, a loading screen, or the window moved.
                    if last_good.elapsed() > Duration::from_secs(3) {
                        crate::diag::log("strip: lost, no frame read at its place for 3 s");
                        locked = None;
                    }
                }
            }
            thread::sleep(Duration::from_millis(80));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcap::image::Rgba;

    /// Rust copy of Strip.lua's encoder with the palette row after the frame, for tests only.
    fn encode(seq: u8, text: &str) -> Vec<u8> {
        let mut bytes = vec![seq, (text.len() >> 8) as u8, text.len() as u8];
        bytes.extend_from_slice(text.as_bytes());
        let sum = fletcher16(&bytes);
        bytes.extend_from_slice(&[(sum >> 8) as u8, sum as u8]);
        let mut symbols = SYNC.to_vec();
        for chunk in bytes.chunks(3) {
            let v = (chunk[0] as u32) << 16
                | (*chunk.get(1).unwrap_or(&0) as u32) << 8
                | *chunk.get(2).unwrap_or(&0) as u32;
            symbols.extend_from_slice(&[(v >> 18) as u8 & 63, (v >> 12) as u8 & 63, (v >> 6) as u8 & 63, v as u8 & 63]);
        }
        symbols.resize(COLS * ROWS, 0);
        symbols.extend(0..COLS as u8);
        symbols
    }

    fn paint(symbols: &[u8], at: (u32, u32), block: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(800, 200, Rgba([30, 30, 40, 255]));
        for (i, s) in symbols.iter().enumerate() {
            let (r, g, b) = ((s >> 4) & 3, (s >> 2) & 3, s & 3);
            for dy in 0..block {
                for dx in 0..block {
                    let x = at.0 + (i % COLS) as u32 * block + dx;
                    let y = at.1 + (i / COLS) as u32 * block + dy;
                    // A small color shift, as a display pipeline might add.
                    img.put_pixel(x, y, Rgba([(r * 85).saturating_add(6), (g * 85).saturating_sub(5), b * 85, 255]));
                }
            }
        }
        img
    }

    #[test]
    fn finds_and_decodes() {
        let text = "7\t[Party] Lili\t副本缺T 速来";
        let img = paint(&encode(42, text), (37, 11), 4);
        let lock = find(&img).expect("strip found");
        assert_eq!(lock, Lock { x: 37, y: 11, block: 4 });
        match decode(&img, lock) {
            Decoded::Frame(f) => {
                assert_eq!(f.seq, 42);
                assert_eq!(f.text, text);
            }
            other => panic!("{other:?}"),
        }
    }

    /// What macOS does on a scale-2 display: full and region captures both
    /// come back in pixels, but regions are requested in points.
    #[test]
    fn rereads_the_strip_through_a_hidpi_region() {
        let text = "7\t[Party] Lili\t副本缺補 密我";
        let full = paint(&encode(9, text), (0, 51), 4);
        let lock = find(&full).expect("strip found in the full capture");
        let (x, y, w, h) = region_in_points(lock, 2.0, 400, 100).expect("region");
        let region = xcap::image::imageops::crop_imm(&full, x * 2, y * 2, w * 2, h * 2).to_image();
        let found = find(&region).expect("strip found in the region capture");
        assert!(matches!(decode(&region, found), Decoded::Frame(f) if f.text == text));
    }

    #[test]
    fn region_stays_on_screen() {
        // Bottom-right corner of a 1920x1080-point screen at scale 2.
        let lock = Lock { x: 3584, y: 2128, block: 4 };
        let (x, y, w, h) = region_in_points(lock, 2.0, 1920, 1080).expect("region");
        assert!(x + w <= 1920 && y + h <= 1080, "{x},{y} {w}x{h}");
    }

    /// What a MacBook's own screen did to the colors, about: the channels mixed into
    /// each other and the middle levels raised (85 and 170 came back near 109 and 187).
    fn through_a_display(img: &RgbaImage) -> RgbaImage {
        let bent = |v: u8| (v as f32 / 255.0).powf(0.78);
        let mut seen = img.clone();
        for pixel in seen.pixels_mut() {
            let [r, g, b] = [bent(pixel[0]), bent(pixel[1]), bent(pixel[2])];
            let mixed = [0.92 * r + 0.06 * g + 0.02 * b, 0.2 * r + 0.78 * g + 0.02 * b, 0.03 * g + 0.97 * b];
            *pixel = Rgba([(mixed[0] * 255.0) as u8, (mixed[1] * 255.0) as u8, (mixed[2] * 255.0) as u8, 255]);
        }
        seen
    }

    #[test]
    fn reads_colors_the_display_changed() {
        let text = "7\t[Party] Lili\t副本缺T 速来";
        let img = through_a_display(&paint(&encode(3, text), (0, 56), 4));
        let lock = find(&img).expect("strip found");
        assert!(matches!(decode(&img, lock), Decoded::Frame(f) if f.text == text));
    }

    /// A capture of the strip on a MacBook's own screen, whose color profile turns
    /// magenta (255, 0, 255) into (234, 51, 247).
    #[test]
    fn reads_a_capture_of_a_real_screen() {
        let bytes = include_bytes!("../tests/fixtures/macbook-screen.png");
        let img = xcap::image::load_from_memory(bytes).expect("fixture").to_rgba8();
        let lock = find(&img).expect("strip found");
        match decode(&img, lock) {
            Decoded::Frame(frame) => {
                assert!(frame.text.starts_with("0\t\tChanged Channel: [1. General - Alterac Mountains]"), "{}", frame.text)
            }
            other => panic!("{other:?}"),
        }
    }

    /// The frame's first data block can look like the yellow sync block before it.
    #[test]
    fn measures_blocks_when_the_data_starts_like_a_sync_block() {
        let mut symbols = encode(3, "1\tA\t你好");
        symbols[3] = 60;
        let img = paint(&symbols, (0, 58), 4);
        assert_eq!(find(&img).map(|lock| lock.block), Some(4));
    }

    #[test]
    fn tells_a_strip_without_its_palette() {
        let symbols = encode(3, "1\tA\t你好");
        let img = paint(&symbols[..COLS * ROWS], (0, 0), 4);
        assert_eq!(decode(&img, Lock { x: 0, y: 0, block: 4 }), Decoded::NoPalette);
    }

    /// Three pixels in the sync colors turn up in the game's scenery; they are no strip.
    #[test]
    fn takes_no_scenery_for_a_strip() {
        let symbols = encode(3, "1\tA\t你好");
        // The sync blocks and what follows, but no palette row under them.
        let scenery = paint(&symbols[..COLS * ROWS], (120, 40), 1);
        assert_eq!(find(&scenery), None);
        // A strip from before the palette is still told, to ask for the addon's update.
        let old = paint(&symbols[..COLS * ROWS], (120, 40), 4);
        assert_eq!(find(&old).map(|lock| lock.block), Some(4));
        // A row under the frame that is not the 64 colors in order is no palette.
        let mut wrong = symbols.clone();
        wrong[COLS * ROWS + 20] = 63;
        let img = paint(&wrong, (0, 0), 2);
        assert_eq!((find(&img), decode(&img, Lock { x: 0, y: 0, block: 2 })), (None, Decoded::NoPalette));
    }

    #[test]
    fn rejects_damaged_frame() {
        let mut symbols = encode(1, "1\tA\t你好");
        symbols[10] ^= 1;
        let img = paint(&symbols, (0, 0), 3);
        assert_eq!(decode(&img, Lock { x: 0, y: 0, block: 3 }), Decoded::Corrupt);
    }
}
