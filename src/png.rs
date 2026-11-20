//! PNGs of 8-bit RGB encoded one fixed way, so the same pixels always give the same file: OP2TestHarness writes its
//! whole-map images with the same encoder (`png::Encode` in its harness.cpp), so the game's image of a moment and the
//! one OP2Imager draws from its state can be compared by file hash.
//!
//! The encoding, which both sides must follow to the bit:
//! - chunks IHDR (colour type 2), one IDAT, IEND; every row filtered with Paeth (filter 4);
//! - a zlib stream (header 78 01, Adler-32 at the end) of dynamic-Huffman deflate blocks of 65536 symbols (the last
//!   one up to 65536 too), each closed by its end-of-block code;
//! - greedy LZ77: at each byte, up to 32 earlier positions on its 3-byte hash's chain, nearest first, at distances
//!   below 32768; the longest match of 3 to 258 bytes wins (the nearest on a tie), else a literal; every position
//!   passed goes on its chain (hash `((b0 << 10) ^ (b1 << 5) ^ b2) & 0x7FFF`);
//! - code lengths from a Huffman tree built from the counts sorted by (count, symbol), joining the two lightest nodes
//!   (a leaf before an inner node of the same weight); while a code is longer than 15 bits (7 for the code-length
//!   code), every count is halved (at least 1) and the tree built again; a tree with fewer than two symbols gets
//!   symbols 0 and 1 counted once; codes are canonical (RFC 1951);
//! - the literal/length and distance lengths are run-length coded as one sequence: zero runs as 18 (11-138) while 11
//!   or more are left, then 17 (3-10), then single zeros; any other length once, then 16 (3-6 repeats) while 3 or
//!   more are left, then singly.

struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, count: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += count;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code, its most significant bit first.
    fn code(&mut self, c: u32, len: u32) {
        let r = if len == 0 { 0 } else { c.reverse_bits() >> (32 - len) };
        self.put(r, len);
    }

    fn flush(&mut self) {
        if self.n > 0 {
            self.put(0, 8 - self.n);
        }
    }
}

fn lengths(mut f: Vec<u32>, limit: u32) -> Vec<u32> {
    if f.iter().filter(|&&v| v != 0).count() < 2 {
        for v in &mut f[..2] {
            if *v == 0 {
                *v = 1;
            }
        }
    }
    loop {
        let mut leaves: Vec<usize> = (0..f.len()).filter(|&s| f[s] != 0).collect();
        leaves.sort_by_key(|&s| f[s]); // stable: equal counts stay in symbol order
        let k = leaves.len();
        // nodes 0..k are the leaves in sorted order, k.. the inner nodes as they are made
        let mut w: Vec<u64> = vec![0; 2 * k - 1];
        let mut parent = vec![0usize; 2 * k - 1];
        for (i, &s) in leaves.iter().enumerate() {
            w[i] = f[s] as u64;
        }
        let (mut li, mut ii, mut made) = (0, k, k);
        while made < 2 * k - 1 {
            let mut take = || {
                if li < k && (ii >= made || w[li] <= w[ii]) {
                    li += 1;
                    li - 1
                } else {
                    ii += 1;
                    ii - 1
                }
            };
            let (a, b) = (take(), take());
            w[made] = w[a] + w[b];
            parent[a] = made;
            parent[b] = made;
            made += 1;
        }
        let mut depth = vec![0u32; 2 * k - 1];
        for i in (0..2 * k - 2).rev() {
            depth[i] = depth[parent[i]] + 1;
        }
        if depth[..k].iter().all(|&d| d <= limit) {
            let mut len = vec![0; f.len()];
            for (i, &s) in leaves.iter().enumerate() {
                len[s] = depth[i];
            }
            return len;
        }
        for v in f.iter_mut().filter(|v| **v != 0) {
            *v = (*v >> 1).max(1);
        }
    }
}

fn codes(len: &[u32]) -> Vec<u32> {
    let mut bl_count = [0u32; 16];
    for &l in len.iter().filter(|&&l| l != 0) {
        bl_count[l as usize] += 1;
    }
    let mut next = [0u32; 16];
    let mut code = 0;
    for b in 1..16 {
        code = (code + bl_count[b - 1]) << 1;
        next[b] = code;
    }
    len.iter()
        .map(|&l| {
            if l == 0 {
                return 0;
            }
            let c = next[l as usize];
            next[l as usize] += 1;
            c
        })
        .collect()
}

const LEN_BASE: [u32; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// The last entry whose base is at most `v`.
fn slot(base: &[u32], v: u32) -> usize {
    base.iter().rposition(|&b| b <= v).unwrap_or(0)
}

/// A literal (`len` 0) or a match.
#[derive(Clone, Copy)]
struct Sym {
    len: u32,
    dist: u32,
    lit: u8,
}

fn block(bits: &mut Bits, syms: &[Sym], last: bool) {
    let (mut lf, mut df) = (vec![0u32; 286], vec![0u32; 30]);
    for s in syms {
        if s.len == 0 {
            lf[s.lit as usize] += 1;
        } else {
            lf[257 + slot(&LEN_BASE, s.len)] += 1;
            df[slot(&DIST_BASE, s.dist)] += 1;
        }
    }
    lf[256] = 1;
    let (ll, dl) = (lengths(lf, 15), lengths(df, 15));
    let (lc, dc) = (codes(&ll), codes(&dl));
    let mut hlit = 286;
    while hlit > 257 && ll[hlit - 1] == 0 {
        hlit -= 1;
    }
    let mut hdist = 30;
    while hdist > 1 && dl[hdist - 1] == 0 {
        hdist -= 1;
    }
    let seq: Vec<u32> = ll[..hlit].iter().chain(&dl[..hdist]).copied().collect();
    let mut rle: Vec<(u32, u32)> = Vec::new(); // (symbol 0-18, its extra bits' value)
    let mut i = 0;
    while i < seq.len() {
        let v = seq[i];
        let mut run = 1;
        while i + run < seq.len() && seq[i + run] == v {
            run += 1;
        }
        let mut r = run as u32;
        if v == 0 {
            while r >= 11 {
                let t = r.min(138);
                rle.push((18, t - 11));
                r -= t;
            }
            if r >= 3 {
                rle.push((17, r - 3));
                r = 0;
            }
        } else {
            rle.push((v, 0));
            r -= 1;
            while r >= 3 {
                let t = r.min(6);
                rle.push((16, t - 3));
                r -= t;
            }
        }
        for _ in 0..r {
            rle.push((v, 0));
        }
        i += run;
    }
    let mut cf = vec![0u32; 19];
    for &(s, _) in &rle {
        cf[s as usize] += 1;
    }
    let cl = lengths(cf, 7);
    let cc = codes(&cl);
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut hclen = 19;
    while hclen > 4 && cl[ORDER[hclen - 1]] == 0 {
        hclen -= 1;
    }
    bits.put(last as u32, 1);
    bits.put(2, 2);
    bits.put(hlit as u32 - 257, 5);
    bits.put(hdist as u32 - 1, 5);
    bits.put(hclen as u32 - 4, 4);
    for &o in &ORDER[..hclen] {
        bits.put(cl[o], 3);
    }
    for &(s, extra) in &rle {
        bits.code(cc[s as usize], cl[s as usize]);
        match s {
            16 => bits.put(extra, 2),
            17 => bits.put(extra, 3),
            18 => bits.put(extra, 7),
            _ => {}
        }
    }
    for s in syms {
        if s.len == 0 {
            bits.code(lc[s.lit as usize], ll[s.lit as usize]);
            continue;
        }
        let (l, d) = (slot(&LEN_BASE, s.len), slot(&DIST_BASE, s.dist));
        bits.code(lc[257 + l], ll[257 + l]);
        bits.put(s.len - LEN_BASE[l], LEN_EXTRA[l]);
        bits.code(dc[d], dl[d]);
        bits.put(s.dist - DIST_BASE[d], DIST_EXTRA[d]);
    }
    bits.code(lc[256], ll[256]);
}

fn deflate(d: &[u8]) -> Vec<u8> {
    const WINDOW: usize = 32768;
    const MASK: usize = WINDOW - 1;
    const CHAIN_MAX: u32 = 32;
    const BLOCK_MAX: usize = 65536;
    let n = d.len();
    let mut head = vec![-1i64; 1 << 15];
    let mut prev = vec![-1i64; WINDOW];
    let hash = |i: usize| (((d[i] as usize) << 10) ^ ((d[i + 1] as usize) << 5) ^ d[i + 2] as usize) & 0x7FFF;
    // puts position i on its hash's chain
    let insert = |head: &mut [i64], prev: &mut [i64], i: usize| {
        if i + 2 >= n {
            return;
        }
        let h = hash(i);
        prev[i & MASK] = head[h];
        head[h] = i as i64;
    };
    let mut bits = Bits { out: Vec::new(), acc: 0, n: 0 };
    let mut syms: Vec<Sym> = Vec::with_capacity(BLOCK_MAX);
    let mut i = 0;
    while i < n {
        let (mut best, mut best_dist) = (0usize, 0usize);
        if i + 2 < n {
            let max_len = 258.min(n - i);
            let mut c = head[hash(i)];
            let mut chain = CHAIN_MAX;
            while c >= 0 && i - (c as usize) < WINDOW && chain > 0 {
                let cu = c as usize;
                let mut len = 0;
                while len < max_len && d[cu + len] == d[i + len] {
                    len += 1;
                }
                if len > best {
                    best = len;
                    best_dist = i - cu;
                    if len == max_len {
                        break;
                    }
                }
                let next = prev[cu & MASK];
                if next >= c {
                    break;
                }
                c = next;
                chain -= 1;
            }
        }
        if best >= 3 {
            syms.push(Sym { len: best as u32, dist: best_dist as u32, lit: 0 });
            for k in 0..best {
                insert(&mut head, &mut prev, i + k);
            }
            i += best;
        } else {
            syms.push(Sym { len: 0, dist: 0, lit: d[i] });
            insert(&mut head, &mut prev, i);
            i += 1;
        }
        if syms.len() == BLOCK_MAX && i < n {
            block(&mut bits, &syms, false);
            syms.clear();
        }
    }
    block(&mut bits, &syms, true);
    bits.flush();
    bits.out
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    !data.iter().fold(!0u32, |crc, &b| table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8))
}

fn chunk(file: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    file.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = file.len();
    file.extend_from_slice(kind);
    file.extend_from_slice(data);
    let crc = crc32(&file[start..]);
    file.extend_from_slice(&crc.to_be_bytes());
}

/// The PNG file of `rgb`: top-down rows of `w` pixels, 3 bytes each (R, G, B).
pub fn encode(rgb: &[u8], w: u32, h: u32) -> Vec<u8> {
    let row = w as usize * 3;
    let mut raw = Vec::with_capacity((row + 1) * h as usize);
    for y in 0..h as usize {
        let cur = &rgb[y * row..(y + 1) * row];
        let up = (y > 0).then(|| &rgb[(y - 1) * row..y * row]);
        raw.push(4);
        for x in 0..row {
            let a = if x >= 3 { cur[x - 3] as i32 } else { 0 };
            let b = up.map_or(0, |u| u[x] as i32);
            let c = match up {
                Some(u) if x >= 3 => u[x - 3] as i32,
                _ => 0,
            };
            let p = a + b - c;
            let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
            let pred = if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c };
            raw.push(cur[x].wrapping_sub(pred as u8));
        }
    }
    let mut z = vec![0x78, 0x01];
    z.extend(deflate(&raw));
    let (mut s1, mut s2) = (1u32, 0u32);
    for &b in &raw {
        s1 = (s1 + b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    z.extend_from_slice(&((s2 << 16) | s1).to_be_bytes());
    let mut file = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut file, b"IHDR", &ihdr);
    chunk(&mut file, b"IDAT", &z);
    chunk(&mut file, b"IEND", &[]);
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The encoder's files decode back to the pixels they were given.
    #[test]
    fn round_trip() {
        let (w, h) = (97u32, 61u32);
        let mut rgb = Vec::new();
        let mut x = 12345u32;
        for i in 0..w * h * 3 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            // a mix of noise and runs, so both literals and matches show up
            rgb.push(if (i / 300) % 2 == 0 { (x >> 16) as u8 } else { (i % 7) as u8 });
        }
        let file = encode(&rgb, w, h);
        let img = image::load_from_memory_with_format(&file, image::ImageFormat::Png).unwrap().into_rgb8();
        assert_eq!(img.dimensions(), (w, h));
        assert_eq!(img.into_raw(), rgb);
    }
}
