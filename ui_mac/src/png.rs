// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! A PNG encoder small enough to read: `--render-to`'s output, with no dependency.
//!
//! Deflate's *stored* blocks rather than compressed ones — a frame is a test artefact, read by a
//! comparison and sometimes by a person, and neither cares about its size. What matters is that
//! the bytes are a function of the pixels alone, so two renders of one document compare equal.

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Encode `rgba` — `width × height` pixels, four bytes each, top row first.
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let stride = width as usize * 4;
    assert_eq!(
        rgba.len(),
        stride * height as usize,
        "one RGBA pixel per place"
    );

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    // Eight bits a channel, colour type 6 (RGBA), deflate, filter method 0, no interlace.
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);

    // Each row behind its filter byte, 0: none.
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for row in rgba.chunks_exact(stride.max(1)).take(height as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }

    let mut out = SIGNATURE.to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A zlib stream of stored deflate blocks, at most 65,535 bytes each.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut blocks = data.chunks(0xFFFF).peekable();
    if blocks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = Crc32::default();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.finish().to_be_bytes());
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// CRC-32 (IEEE), bit by bit: a frame is checksummed once, so a table would be the slow part.
struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Crc32(0xFFFF_FFFF)
    }
}

impl Crc32 {
    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.0 ^= u32::from(byte);
            for _ in 0..8 {
                let mask = (self.0 & 1).wrapping_neg();
                self.0 = (self.0 >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
    }

    fn finish(self) -> u32 {
        !self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksums_are_the_standard_ones() {
        let mut crc = Crc32::default();
        crc.update(b"123456789");
        assert_eq!(crc.finish(), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn a_png_is_signature_header_data_end() {
        let png = encode(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]);
        assert_eq!(&png[..8], &SIGNATURE);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 2);
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }

    #[test]
    fn a_frame_larger_than_one_stored_block_is_split() {
        let data = vec![7u8; 70_000];
        let z = zlib_stored(&data);
        // Header, two block headers of five bytes, the data, the checksum.
        assert_eq!(z.len(), 2 + 5 + 5 + data.len() + 4);
        assert_eq!(z[2], 0, "the first block is not final");
    }
}
