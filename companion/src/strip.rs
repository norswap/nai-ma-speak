//! Reads the addon's pixel strip off the screen. The layout must match
//! addon/Translate/Strip.lua: 64 x 8 blocks, 3 sync blocks, then
//! [seq][len hi][len lo][text...][fletcher16 hi][fletcher16 lo] packed
//! 3 bytes into 4 six-bit blocks.

use crate::worker::{Job, UiEvent, UiSender};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};
use xcap::image::RgbaImage;
use xcap::Monitor;

pub const COLS: usize = 64;
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
    /// Sync blocks present but the data did not check out (mid-repaint).
    Corrupt,
    /// No strip at this spot.
    Missing,
}

/// Maps a channel value to one of the four levels the addon draws (0, 85, 170, 255).
fn level(v: f32) -> u8 {
    ((v + 42.0) / 85.0).clamp(0.0, 3.0) as u8
}

/// The pixel's channels, or None outside the image.
fn pixel(img: &RgbaImage, x: u32, y: u32) -> Option<[f32; 3]> {
    if x >= img.width() || y >= img.height() {
        return None;
    }
    let p = img.get_pixel(x, y).0;
    Some([p[0] as f32, p[1] as f32, p[2] as f32])
}

/// Reads a sync color using only on/off per channel, which survives a display
/// color profile that shifts the four levels into each other.
fn sync_symbol(img: &RgbaImage, x: u32, y: u32) -> Option<u8> {
    let [r, g, b] = pixel(img, x, y)?;
    let on = |v: f32| if v >= 128.0 { 3 } else { 0 };
    Some(on(r) * 16 + on(g) * 4 + on(b))
}

fn to_linear(v: f32) -> f32 {
    (v / 255.0).powi(2)
}

fn invert(m: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let cofactor = |r: usize, c: usize| {
        let (r1, r2, c1, c2) = ((r + 1) % 3, (r + 2) % 3, (c + 1) % 3, (c + 2) % 3);
        m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]
    };
    let det = m[0][0] * cofactor(0, 0) + m[0][1] * cofactor(0, 1) + m[0][2] * cofactor(0, 2);
    if det.abs() < 1e-3 {
        return None;
    }
    let mut inverse = [[0.0; 3]; 3];
    for (r, row) in inverse.iter_mut().enumerate() {
        for (c, value) in row.iter_mut().enumerate() {
            *value = cofactor(c, r) / det;
        }
    }
    Some(inverse)
}

/// macOS screen capture returns pixels in the display's color space, so a
/// monitor profile mixes the channels: on a Dell SE2717H, yellow (255, 255, 0)
/// reads back as (255, 253, 45). The sync blocks are known colors, so they
/// show how each channel was mixed, and undoing that mix recovers the colors
/// the addon drew. Ported from github.com/hola-ai/Translate (MIT).
fn calibration(sync: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let [magenta, cyan, yellow] = sync.map(|p| p.map(to_linear));
    let mut primaries = [[0.0; 3]; 3];
    for (channel, row) in primaries.iter_mut().enumerate() {
        let (m, c, y) = (magenta[channel], cyan[channel], yellow[channel]);
        *row = [(m + y - c) / 2.0, (c + y - m) / 2.0, (m + c - y) / 2.0];
    }
    invert(primaries)
}

fn symbol(inverse: &[[f32; 3]; 3], pixel: [f32; 3]) -> u8 {
    let linear = pixel.map(to_linear);
    let channel = |row: [f32; 3]| {
        let mixed = row[0] * linear[0] + row[1] * linear[1] + row[2] * linear[2];
        level(mixed.max(0.0).sqrt() * 255.0)
    };
    channel(inverse[0]) * 16 + channel(inverse[1]) * 4 + channel(inverse[2])
}

fn fletcher16(bytes: &[u8]) -> u16 {
    let (mut a, mut b) = (0u16, 0u16);
    for &byte in bytes {
        a = (a + byte as u16) % 255;
        b = (b + a) % 255;
    }
    b * 256 + a
}

pub fn decode(img: &RgbaImage, lock: Lock) -> Decoded {
    let center = |i: usize| {
        (
            lock.x + (i % COLS) as u32 * lock.block + lock.block / 2,
            lock.y + (i / COLS) as u32 * lock.block + lock.block / 2,
        )
    };
    let mut sync = [[0.0; 3]; 3];
    for (i, expected) in SYNC.iter().enumerate() {
        let (cx, cy) = center(i);
        if sync_symbol(img, cx, cy) != Some(*expected) {
            return Decoded::Missing;
        }
        sync[i] = pixel(img, cx, cy).expect("sync_symbol found the pixel");
    }
    let calibrated = match calibration(sync) {
        Some(inverse) => read_frame(img, &center, &inverse),
        None => Decoded::Corrupt,
    };
    if calibrated != Decoded::Corrupt {
        return calibrated;
    }
    // Calibrating from three pixels amplifies noise near a level boundary (a
    // lossy screenshot, say), so a frame that fails its checksum is read once
    // more without it; that read is exactly what the strip reader did before.
    match read_frame(img, &center, &IDENTITY) {
        Decoded::Frame(frame) => Decoded::Frame(frame),
        _ => Decoded::Corrupt,
    }
}

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn read_frame(img: &RgbaImage, center: &dyn Fn(usize) -> (u32, u32), inverse: &[[f32; 3]; 3]) -> Decoded {
    let mut symbols = Vec::with_capacity(COLS * ROWS);
    for i in 0..COLS * ROWS {
        let (cx, cy) = center(i);
        match pixel(img, cx, cy) {
            Some(p) => symbols.push(symbol(inverse, p)),
            None => return Decoded::Missing,
        }
    }
    let mut bytes = Vec::with_capacity((symbols.len() - 3) / 4 * 3);
    for chunk in symbols[3..].chunks_exact(4) {
        let v = (chunk[0] as u32) << 18 | (chunk[1] as u32) << 12 | (chunk[2] as u32) << 6 | chunk[3] as u32;
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
            // Averaging the three runs tolerates a blended edge pixel.
            let block = ((block + cyan + yellow) as f32 / 3.0).round() as u32;
            let mut top = y;
            while top > 0 && is(start, top - 1, SYNC[0]) {
                top -= 1;
            }
            let lock = Lock { x: start, y: top, block: block.max(1) };
            if decode(img, lock) != Decoded::Missing {
                return Some(lock);
            }
        }
    }
    None
}

/// The payload is "<id>\t<speaker>\t<text>".
pub fn parse_payload(text: &str) -> Option<(String, String)> {
    let mut parts = text.splitn(3, '\t');
    let _id = parts.next()?;
    let speaker = parts.next()?.to_string();
    let message = parts.next()?.to_string();
    Some((speaker, message))
}

/// Polls the screen: a full scan about once a second until the strip turns
/// up, then only the strip's own rectangle several times a second.
///
/// Set TRANSLATE_DUMP=1 to save each full scan as translate-capture-<n>.png
/// in the working directory and log what was captured, for `--decode`.
pub fn spawn(jobs: Sender<Job>, ui: UiSender) {
    thread::spawn(move || {
        let mut locked: Option<(usize, Lock)> = None;
        let mut last_seen: Option<(u8, u16)> = None;
        let mut last_good = Instant::now();
        let mut said_searching = false;
        let mut said_capture_error = false;
        let dump = std::env::var_os("TRANSLATE_DUMP").is_some();

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
                if !said_searching {
                    ui.send(UiEvent::Strip("Looking for the pixel strip. In game, type /tr test.".into()));
                    said_searching = true;
                }
                for (index, monitor) in monitors.iter().enumerate() {
                    let img = match monitor.capture_image() {
                        Ok(img) => img,
                        Err(e) => {
                            // Used to be skipped silently, which looks exactly
                            // like "no strip on screen".
                            if !said_capture_error {
                                eprintln!("screen {}: capture failed: {e}", index + 1);
                                ui.send(UiEvent::Strip(format!("Screen {} capture failed: {e}", index + 1)));
                                said_capture_error = true;
                            }
                            continue;
                        }
                    };
                    if dump {
                        let path = format!("translate-capture-{}.png", index + 1);
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
                        eprintln!("screen {}: strip found at {},{} ({} px blocks)", index + 1, lock.x, lock.y, lock.block);
                        ui.send(UiEvent::Strip(format!(
                            "Reading the pixel strip on screen {} at {},{}",
                            index + 1,
                            lock.x,
                            lock.y
                        )));
                        locked = Some((index, lock));
                        last_good = Instant::now();
                        said_searching = false;
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
            let (w, h) = (COLS as u32 * lock.block, ROWS as u32 * lock.block);
            let decoded = match monitor.capture_region(lock.x, lock.y, w, h) {
                Ok(img) => decode(&img, Lock { x: 0, y: 0, block: lock.block }),
                Err(_) => Decoded::Missing,
            };
            match decoded {
                Decoded::Frame(frame) => {
                    last_good = Instant::now();
                    if last_seen != Some((frame.seq, frame.checksum)) {
                        // The first frame after locking may be an old line; it is
                        // still new to the overlay, so it is shown too.
                        last_seen = Some((frame.seq, frame.checksum));
                        if let Some((speaker, text)) = parse_payload(&frame.text) {
                            let _ = jobs.send(Job::Incoming { speaker, text });
                        }
                    }
                }
                Decoded::Corrupt => {}
                Decoded::Missing => {
                    // Hidden UI, a loading screen, or the window moved.
                    if last_good.elapsed() > Duration::from_secs(3) {
                        eprintln!("lost the strip: no frame read at the locked spot for 3 s");
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

    /// Rust copy of Strip.lua's encoder, for tests only.
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
        assert_eq!(parse_payload(text), Some(("[Party] Lili".into(), "副本缺T 速来".into())));
    }

    fn fixture(bytes: &[u8]) -> RgbaImage {
        xcap::image::load_from_memory(bytes).expect("fixture").to_rgba8()
    }

    fn assert_test_line(img: &RgbaImage) {
        let lock = find(img).expect("strip found");
        match decode(img, lock) {
            Decoded::Frame(f) => assert!(f.text.ends_with("大家好 (Translate test)"), "{}", f.text),
            other => panic!("{other:?}"),
        }
    }

    /// A macOS screenshot of the strip on a Dell SE2717H. The display profile
    /// turns yellow (255, 255, 0) into (255, 253, 45), so the old reader never
    /// recognised the sync blocks.
    #[test]
    fn decodes_a_capture_through_a_monitor_profile() {
        assert_test_line(&fixture(include_bytes!("../tests/fixtures/dell-se2717h-macos.png")));
    }

    /// WoW's own screenshot, saved as JPEG: calibrating amplifies the
    /// compression noise, so the plain read has to take over.
    #[test]
    fn falls_back_to_the_plain_read() {
        assert_test_line(&fixture(include_bytes!("../tests/fixtures/ingame-jpeg.png")));
    }

    #[test]
    fn rejects_damaged_frame() {
        let mut symbols = encode(1, "1\tA\t你好");
        symbols[10] ^= 1;
        let img = paint(&symbols, (0, 0), 3);
        assert_eq!(decode(&img, Lock { x: 0, y: 0, block: 3 }), Decoded::Corrupt);
    }
}
