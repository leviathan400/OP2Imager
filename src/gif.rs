//! Animated GIFs of images of one size, for timelapses of OP2TestHarness's whole-map images (`OP2Timelapse`).
//!
//! A GIF holds 256 colours, so the frames share one palette: 253 colours chosen by median cut over their 15-bit
//! colours (the game draws in 15-bit), then black and white for the labels and one transparent index. Median cut
//! splits the group of colours whose widest channel range, times the square root of its weight, is largest; a colour's
//! weight is the square root of its pixel count, so a few rare, saturated colours (team colours, lights, lava) keep
//! entries of their own instead of going to the nearest terrain colour. Every frame after the first stores only the
//! pixels that changed; the rest are transparent and show the frame before.

use image::RgbImage;

/// The label bar above each frame, in pixels.
const BAR: u32 = 22;
const TRANSPARENT: u8 = 255;
const WHITE: u8 = 254;
const BLACK: u8 = 253;
/// The palette's colours chosen from the frames.
const CHOSEN: usize = 253;

/// How a timelapse is laid out and timed.
pub struct Options {
    /// Each frame's time on screen, in hundredths of a second.
    pub delay: u16,
    /// The last frame's, before the loop starts again.
    pub last_delay: u16,
}

/// What `encode` made.
pub struct Encoded {
    pub gif: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The mean distance of a pixel from its palette colour (largest channel difference, 0-255), and how many pixels
    /// are more than 24 levels off.
    pub mean_error: f64,
    pub far_off: u64,
    pub pixels: u64,
}

/// A 3x5 glyph, a row a byte, its 3 low bits the pixels left to right; unknown characters are blank.
fn glyph(c: char) -> [u8; 5] {
    match c.to_ascii_uppercase() {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 7, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 1, 1, 1],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 6, 3],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '.' => [0, 0, 0, 0, 2],
        ',' => [0, 0, 0, 2, 4],
        ':' => [0, 2, 0, 2, 0],
        '-' => [0, 0, 7, 0, 0],
        '_' => [0, 0, 0, 0, 7],
        '/' => [1, 1, 2, 4, 4],
        '(' => [1, 2, 2, 2, 1],
        ')' => [4, 2, 2, 2, 4],
        '#' => [5, 7, 5, 7, 5],
        _ => [0; 5],
    }
}

/// Writes `s` in white at (x0, y0), each glyph pixel `scale` pixels square, cut at the right edge.
fn text(px: &mut [u8], w: u32, x0: u32, y0: u32, s: &str, scale: u32) {
    for (i, c) in s.chars().enumerate() {
        for (row, bits) in glyph(c).iter().enumerate() {
            for col in (0..3).filter(|col| bits >> (2 - col) & 1 == 1) {
                for dy in 0..scale {
                    for dx in 0..scale {
                        let x = x0 + (i as u32 * 4 + col) * scale + dx;
                        if x < w {
                            px[((y0 + row as u32 * scale + dy) * w + x) as usize] = WHITE;
                        }
                    }
                }
            }
        }
    }
}

/// GIF's variable-length LZW of 8-bit indices (minimum code size 8), starting with a clear code and starting again
/// with one whenever the 4096 codes are used up.
fn lzw(data: &[u8]) -> Vec<u8> {
    use std::collections::HashMap;
    const CLEAR: u32 = 256;
    const END: u32 = 257;
    let mut out = Vec::new();
    let (mut acc, mut n) = (0u32, 0u32);
    let mut put = |out: &mut Vec<u8>, code: u32, size: u32| {
        acc |= code << n;
        n += size;
        while n >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            n -= 8;
        }
    };
    let mut dict: HashMap<(u32, u8), u32> = HashMap::new();
    let (mut next, mut size) = (END + 1, 9);
    put(&mut out, CLEAR, size);
    let mut cur: Option<u32> = None;
    for &b in data {
        let Some(c) = cur else {
            cur = Some(b as u32);
            continue;
        };
        if let Some(&code) = dict.get(&(c, b)) {
            cur = Some(code);
            continue;
        }
        put(&mut out, c, size);
        if next < 4096 {
            dict.insert((c, b), next);
            next += 1;
            if next > 1 << size && size < 12 {
                size += 1;
            }
        } else {
            put(&mut out, CLEAR, size);
            dict.clear();
            (next, size) = (END + 1, 9);
        }
        cur = Some(b as u32);
    }
    if let Some(c) = cur {
        put(&mut out, c, size);
    }
    put(&mut out, END, size);
    if n > 0 {
        out.push(acc as u8);
    }
    out
}

/// The 15-bit colour of a pixel, as an index (5 bits each of red, green, blue).
fn key(p: &image::Rgb<u8>) -> usize {
    ((p[0] as usize >> 3) << 10) | ((p[1] as usize >> 3) << 5) | (p[2] as usize >> 3)
}

/// A 15-bit colour back to 8 bits a channel, as the game widens it.
fn colour(k: usize) -> [i32; 3] {
    let widen = |c5: usize| ((c5 << 3) | (c5 >> 2)) as i32;
    [widen(k >> 10), widen((k >> 5) & 31), widen(k & 31)]
}

/// The palette (median cut, see the module) and, for every 15-bit colour the frames use, its palette index.
fn palette(frames: &[RgbImage]) -> (Vec<[i32; 3]>, Vec<u8>) {
    let mut hist = vec![0u64; 32768];
    for p in frames.iter().flat_map(|f| f.pixels()) {
        hist[key(p)] += 1;
    }
    let weight = |k: usize| (hist[k] as f64).sqrt();
    let used: Vec<usize> = (0..32768).filter(|&k| hist[k] > 0).collect();
    let mut boxes: Vec<Vec<usize>> = vec![used.clone()];
    while boxes.len() < CHOSEN {
        // (box, channel, score) of the split to make
        let mut pick: Option<(usize, usize, f64)> = None;
        for (i, bx) in boxes.iter().enumerate().filter(|(_, b)| b.len() > 1) {
            let total: f64 = bx.iter().map(|&k| weight(k)).sum();
            for ch in 0..3 {
                let (lo, hi) = bx.iter().fold((255, 0), |(lo, hi), &k| (colour(k)[ch].min(lo), colour(k)[ch].max(hi)));
                let score = (hi - lo) as f64 * total.sqrt();
                if hi > lo && pick.is_none_or(|(_, _, s)| score > s) {
                    pick = Some((i, ch, score));
                }
            }
        }
        let Some((i, ch, _)) = pick else { break };
        let mut bx = boxes.swap_remove(i);
        bx.sort_by_key(|&k| (colour(k)[ch], k));
        // at the weighted median, keeping a colour on each side
        let total: f64 = bx.iter().map(|&k| weight(k)).sum();
        let (mut acc, mut cut) = (0.0, bx.len() / 2);
        for (j, &k) in bx.iter().enumerate() {
            acc += weight(k);
            if acc >= total / 2.0 {
                cut = (j + 1).clamp(1, bx.len() - 1);
                break;
            }
        }
        let rest = bx.split_off(cut);
        boxes.push(bx);
        boxes.push(rest);
    }
    // each box's colour: its pixels' mean
    let pal: Vec<[i32; 3]> = boxes
        .iter()
        .map(|bx| {
            let n: f64 = bx.iter().map(|&k| hist[k] as f64).sum();
            let mean = |ch: usize| (bx.iter().map(|&k| colour(k)[ch] as f64 * hist[k] as f64).sum::<f64>() / n).round() as i32;
            [mean(0), mean(1), mean(2)]
        })
        .collect();
    let mut lut = vec![0u8; 32768];
    for &k in &used {
        let c = colour(k);
        let dist = |p: &[i32; 3]| (0..3).map(|j| (p[j] - c[j]).pow(2)).sum::<i32>();
        lut[k] = (0..pal.len()).min_by_key(|&i| dist(&pal[i])).unwrap_or(0) as u8;
    }
    (pal, lut)
}

/// An animated GIF of `frames` (all one size), looping, each under a bar with its label (none when `labels` is
/// empty).
pub fn encode(frames: &[RgbImage], labels: &[String], o: &Options) -> Encoded {
    let (w, h) = frames.first().map_or((0, 0), |f| f.dimensions());
    let bar = if labels.is_empty() { 0 } else { BAR };
    let fh = h + bar;
    let (pal, lut) = palette(frames);
    let (mut err_sum, mut far_off, mut pixels) = (0f64, 0u64, 0u64);
    for p in frames.iter().flat_map(|f| f.pixels()) {
        let q = pal[lut[key(p)] as usize];
        let e = (0..3).map(|j| (q[j] - p[j] as i32).abs()).max().unwrap_or(0);
        err_sum += e as f64;
        far_off += u64::from(e > 24);
        pixels += 1;
    }

    let mut gif = Vec::new();
    gif.extend_from_slice(b"GIF89a");
    gif.extend_from_slice(&(w as u16).to_le_bytes());
    gif.extend_from_slice(&(fh as u16).to_le_bytes());
    gif.extend_from_slice(&[0xF7, 0, 0]); // a global colour table of 256, 8 bits a channel
    for i in 0..256 {
        let c = match i as u8 {
            WHITE => [255, 255, 255],
            BLACK | TRANSPARENT => [0, 0, 0],
            _ => pal.get(i).copied().unwrap_or([0, 0, 0]),
        };
        gif.extend(c.iter().map(|&v| v as u8));
    }
    // loop forever (the NETSCAPE2.0 application extension)
    gif.extend_from_slice(&[0x21, 0xFF, 11]);
    gif.extend_from_slice(b"NETSCAPE2.0");
    gif.extend_from_slice(&[3, 1, 0, 0, 0]);
    let mut prev: Option<Vec<u8>> = None;
    for (i, f) in frames.iter().enumerate() {
        let mut idx = vec![BLACK; (w * fh) as usize];
        for (x, y, p) in f.enumerate_pixels() {
            idx[((y + bar) * w + x) as usize] = lut[key(p)];
        }
        if let Some(label) = labels.get(i) {
            text(&mut idx, w, 6, 4, label, 3);
        }
        let mut data = idx.clone();
        if let Some(p) = &prev {
            for (d, &q) in data.iter_mut().zip(p) {
                if *d == q {
                    *d = TRANSPARENT;
                }
            }
        }
        let delay = if i + 1 == frames.len() { o.last_delay } else { o.delay };
        // graphic control: disposal 1 (keep the frame), transparency on frames after the first
        gif.extend_from_slice(&[0x21, 0xF9, 4, (1 << 2) | u8::from(prev.is_some())]);
        gif.extend_from_slice(&delay.to_le_bytes());
        gif.extend_from_slice(&[TRANSPARENT, 0]);
        // the image: the whole frame, no local colour table
        gif.push(0x2C);
        gif.extend_from_slice(&[0, 0, 0, 0]);
        gif.extend_from_slice(&(w as u16).to_le_bytes());
        gif.extend_from_slice(&(fh as u16).to_le_bytes());
        gif.push(0);
        gif.push(8);
        for chunk in lzw(&data).chunks(255) {
            gif.push(chunk.len() as u8);
            gif.extend_from_slice(chunk);
        }
        gif.push(0);
        prev = Some(idx);
    }
    gif.push(0x3B);
    Encoded { gif, width: w, height: fh, mean_error: err_sum / pixels.max(1) as f64, far_off, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain GIF LZW decoder (minimum code size 8), to check `lzw` against.
    fn unlzw(bytes: &[u8]) -> Vec<u8> {
        let (mut acc, mut n, mut pos) = (0u32, 0u32, 0usize);
        let mut table: Vec<Vec<u8>> = Vec::new();
        let reset = |t: &mut Vec<Vec<u8>>| {
            *t = (0..=255u8).map(|b| vec![b]).collect();
            t.push(Vec::new());
            t.push(Vec::new());
        };
        reset(&mut table);
        let mut size = 9;
        let mut out = Vec::new();
        let mut prev: Option<Vec<u8>> = None;
        loop {
            while n < size {
                acc |= (bytes[pos] as u32) << n;
                pos += 1;
                n += 8;
            }
            let code = (acc & ((1 << size) - 1)) as usize;
            acc >>= size;
            n -= size;
            if code == 256 {
                reset(&mut table);
                size = 9;
                prev = None;
                continue;
            }
            if code == 257 {
                return out;
            }
            let entry = if code < table.len() {
                table[code].clone()
            } else {
                let p = prev.clone().unwrap();
                let mut e = p.clone();
                e.push(p[0]);
                e
            };
            if let Some(p) = prev {
                let mut e = p;
                e.push(entry[0]);
                table.push(e);
                if table.len() == 1 << size && size < 12 {
                    size += 1;
                }
            }
            out.extend_from_slice(&entry);
            prev = Some(entry);
        }
    }

    /// The LZW stream decodes back, across dictionary resets.
    #[test]
    fn lzw_round_trip() {
        let mut data = Vec::new();
        let mut x = 7u32;
        for i in 0..200_000u32 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            data.push(if (i / 500) % 2 == 0 { (x >> 16) as u8 } else { (i % 5) as u8 });
        }
        assert_eq!(unlzw(&lzw(&data)), data);
    }

    /// Rare, saturated colours keep their own palette entries next to a mass of terrain colours.
    #[test]
    fn rare_colours_kept() {
        let mut img = RgbImage::new(200, 200);
        for (x, y, p) in img.enumerate_pixels_mut() {
            // 625 15-bit colours, more than the palette holds
            *p = image::Rgb([x as u8, y as u8, 60]);
        }
        img.put_pixel(0, 0, image::Rgb([248, 0, 0]));
        img.put_pixel(1, 0, image::Rgb([0, 0, 248]));
        let e = encode(&[img], &[], &Options { delay: 10, last_delay: 10 });
        assert_eq!(e.far_off, 0);
    }
}
