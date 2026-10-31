//! Blank pictures: an image of the same format, pixel size, resolution,
//! colour mode and rotation tag, with nothing in it.
//!
//! A picture is the author's work and may show anything, so a scrambled copy
//! keeps none of it; but the importer and the typesetter read a picture's
//! format, size, resolution and orientation, so the copy keeps those.
//!
//! | Format | The copy |
//! |---|---|
//! | PNG | Same size, colour type and bit depth, `pHYs` and the orientation (`eXIf`). Light grey (white at 1, 2 and 4 bits; an indexed image gets a one-colour palette). Written without interlacing; every other chunk (text, gamma, ICC profile, transparency) is dropped |
//! | JPEG | Baseline, same size and component count (grey, YCbCr, CMYK), JFIF density, Exif orientation, Adobe marker for four components. Mid-grey; progressive becomes baseline; all other segments (comments, Exif, ICC, XMP) are dropped |
//! | GIF | Same logical screen, one light grey frame (an animation keeps its first frame's place, nothing else) |
//! | WebP | Lossless, same size, with an alpha channel when the original has one, and the orientation. Light grey |
//! | SVG | The root element with its size attributes (`width`, `height`, `viewBox`, `preserveAspectRatio`) and a light grey rectangle over all of it |
//! | Others (TIFF, BMP, EMF, WMF, unknown) | The first 128 bytes (the header the importer sniffs the format from), zeros after, so the length and the "unsupported format" warning stay and the picture does not |
//!
//! The grey is there on purpose: a white picture on a white page cannot be
//! seen, and where a picture stands and how large it is are what layout
//! problems are about.
//!
//! A picture of more than `MAX_PIXELS` pixels (GIF, WebP) or a PNG of more
//! than `MAX_STREAM_PIXELS` pixels or `MAX_ROW_BYTES` a row cannot be blanked
//! and is an error: the app then offers the original.

use std::borrow::Cow;
use std::io::Write;

use setzer_model::asset::ImageFormat;

/// The most pixels of a GIF or WebP we write (their encoders take the
/// whole picture in memory).
const MAX_PIXELS: u64 = 32 * 1024 * 1024;
/// The most pixels of a PNG or JPEG we write (they are written row by row;
/// time grows with it, memory does not).
const MAX_STREAM_PIXELS: u64 = 1 << 28;
/// The longest row of a PNG we write, in bytes. The encoder's stream writer
/// holds three rows before it writes a byte, so a header that claims a row
/// of 400 MB (50,000,000 × 1 at RGBA16, in 65 bytes) would cost 1.2 GB. A
/// real picture of 2 million pixels across at 16-bit RGBA is under this.
const MAX_ROW_BYTES: u64 = 16 * 1024 * 1024;
/// Bytes of an unsupported picture's header that stay.
const HEADER_KEPT: usize = 128;
/// The grey of a blank picture, 0 to 255.
const GREY: u8 = 0xcc;

/// The blank form of the picture in `b`, or why it cannot be made.
pub(super) fn blank(b: &[u8]) -> Result<Vec<u8>, String> {
    match ImageFormat::sniff(b) {
        ImageFormat::Supported { media_type, .. } => match media_type {
            "image/png" => png(b),
            "image/jpeg" => jpeg(b),
            "image/gif" => gif(b),
            "image/webp" => webp(b),
            _ => svg(b),
        },
        ImageFormat::Unsupported(_) => Ok(header_only(b)),
    }
}

fn header_only(b: &[u8]) -> Vec<u8> {
    let mut out = b[..b.len().min(HEADER_KEPT)].to_vec();
    out.resize(b.len(), 0);
    out
}

fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

fn be16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn le16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

// ------------------------------------------------------------ orientation

/// The Exif orientation (1..=8) in a TIFF block, if it says one.
fn orientation(tiff: &[u8]) -> Option<u16> {
    let big = match tiff.get(..2)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let u16_at = |i: usize| {
        let s: [u8; 2] = tiff.get(i..i + 2)?.try_into().ok()?;
        Some(if big {
            u16::from_be_bytes(s)
        } else {
            u16::from_le_bytes(s)
        })
    };
    let u32_at = |i: usize| {
        let s: [u8; 4] = tiff.get(i..i + 4)?.try_into().ok()?;
        Some(if big {
            u32::from_be_bytes(s)
        } else {
            u32::from_le_bytes(s)
        })
    };
    let ifd = u32_at(4)? as usize;
    (0..u16_at(ifd)? as usize).find_map(|n| {
        let entry = ifd + 2 + 12 * n;
        (u16_at(entry)? == 0x0112)
            .then(|| u16_at(entry + 8))
            .flatten()
            .filter(|o| (1..=8).contains(o))
    })
}

/// A TIFF block with an orientation and nothing else (big-endian).
fn orientation_tiff(o: u16) -> Vec<u8> {
    let mut t = b"MM\0*\0\0\0\x08".to_vec();
    t.extend_from_slice(&1u16.to_be_bytes()); // one entry
    t.extend_from_slice(&0x0112u16.to_be_bytes());
    t.extend_from_slice(&3u16.to_be_bytes()); // SHORT
    t.extend_from_slice(&1u32.to_be_bytes());
    t.extend_from_slice(&o.to_be_bytes());
    t.extend_from_slice(&[0, 0]);
    t.extend_from_slice(&0u32.to_be_bytes()); // no next IFD
    t
}

// ------------------------------------------------------------------- PNG

fn png(b: &[u8]) -> Result<Vec<u8>, String> {
    if b.get(12..16) != Some(b"IHDR") {
        return Err("a PNG without a header".into());
    }
    let (w, h) = (
        be32(b, 16).ok_or("short PNG")?,
        be32(b, 20).ok_or("short PNG")?,
    );
    let (depth, color) = (
        *b.get(24).ok_or("short PNG")?,
        *b.get(25).ok_or("short PNG")?,
    );
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_STREAM_PIXELS {
        return Err(format!("a PNG of {w}×{h} pixels"));
    }
    // Reject a row that would not fit before the encoder allocates for it.
    let bits_per_pixel = match (color, depth) {
        (0, 1 | 2 | 4 | 8 | 16) | (3, 1 | 2 | 4 | 8) => u64::from(depth),
        (2, 8 | 16) => 3 * u64::from(depth),
        (4, 8 | 16) => 2 * u64::from(depth),
        (6, 8 | 16) => 4 * u64::from(depth),
        // A combination the checks below name.
        _ => u64::from(depth),
    };
    if (bits_per_pixel * u64::from(w)).div_ceil(8) > MAX_ROW_BYTES {
        return Err(format!("a PNG row of {w} pixels"));
    }
    let mut dims = None;
    let mut exif = None;
    let mut i = 8;
    while let (Some(len), Some(kind)) = (be32(b, i), b.get(i + 4..i + 8)) {
        let data = b.get(i + 8..i + 8 + len as usize);
        match (kind, data) {
            (b"pHYs", Some(d)) if d.len() == 9 => {
                dims = Some(::png::PixelDimensions {
                    xppu: be32(d, 0).unwrap_or(0),
                    yppu: be32(d, 4).unwrap_or(0),
                    unit: if d[8] == 1 {
                        ::png::Unit::Meter
                    } else {
                        ::png::Unit::Unspecified
                    },
                });
            }
            (b"eXIf", Some(d)) => exif = orientation(d),
            (b"IEND", _) => break,
            _ => {}
        }
        i = i
            .checked_add(12 + len as usize)
            .ok_or("a PNG chunk too long")?;
    }

    let color = match color {
        0 => ::png::ColorType::Grayscale,
        2 => ::png::ColorType::Rgb,
        3 => ::png::ColorType::Indexed,
        4 => ::png::ColorType::GrayscaleAlpha,
        6 => ::png::ColorType::Rgba,
        n => return Err(format!("a PNG colour type {n}")),
    };
    let bit_depth = match depth {
        1 => ::png::BitDepth::One,
        2 => ::png::BitDepth::Two,
        4 => ::png::BitDepth::Four,
        8 => ::png::BitDepth::Eight,
        16 => ::png::BitDepth::Sixteen,
        n => return Err(format!("a PNG bit depth {n}")),
    };
    let mut info = ::png::Info::with_size(w, h);
    info.color_type = color;
    info.bit_depth = bit_depth;
    info.pixel_dims = dims;
    if color == ::png::ColorType::Indexed {
        info.palette = Some(Cow::Owned(vec![GREY, GREY, GREY]));
    }
    info.exif_metadata = exif.map(|o| Cow::Owned(orientation_tiff(o)));

    let mut out = Vec::new();
    let encoder = ::png::Encoder::with_info(&mut out, info).map_err(|e| e.to_string())?;
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    // A row is one pattern repeated: light grey, opaque, one pixel at a
    // time; at 1 to 4 bits white, a byte at a time (an indexed picture's
    // index 0 is its grey). The row is at most `MAX_ROW_BYTES`, so one row
    // is built and written `h` times; a header that claims a very tall
    // picture costs time, not memory.
    let (pattern, row_len): (Vec<u8>, u64) = if depth >= 8 {
        let alpha_at = match color {
            ::png::ColorType::GrayscaleAlpha => Some(1),
            ::png::ColorType::Rgba => Some(3),
            _ => None,
        };
        let pixel: Vec<u8> = (0..color.samples())
            .flat_map(|s| {
                // An indexed picture's one sample is the palette index 0,
                // its grey; anything past the palette is an invalid PNG.
                let v = if alpha_at == Some(s) {
                    0xff
                } else if color == ::png::ColorType::Indexed {
                    0
                } else {
                    GREY
                };
                vec![v; usize::from(depth) / 8]
            })
            .collect();
        let len = pixel.len() as u64 * u64::from(w);
        (pixel, len)
    } else {
        let fill = if color == ::png::ColorType::Indexed {
            0
        } else {
            0xff
        };
        (vec![fill], (u64::from(depth) * u64::from(w)).div_ceil(8))
    };
    let row = pattern
        .repeat(usize::try_from(row_len / pattern.len().max(1) as u64).map_err(|e| e.to_string())?);
    let mut stream = writer.stream_writer().map_err(|e| e.to_string())?;
    for _ in 0..h {
        stream.write_all(&row).map_err(|e| e.to_string())?;
    }
    stream.finish().map_err(|e| e.to_string())?;
    drop(writer);
    Ok(out)
}

// ------------------------------------------------------------------ JPEG

fn jpeg(b: &[u8]) -> Result<Vec<u8>, String> {
    let mut jfif = None;
    let mut exif = None;
    let mut adobe = None;
    let mut frame = None;
    let mut i = 2;
    loop {
        if b.get(i) != Some(&0xff) {
            return Err("a damaged JPEG".into());
        }
        let marker = *b.get(i + 1).ok_or("a truncated JPEG")?;
        if marker == 0xff {
            i += 1;
            continue;
        }
        let len = usize::from(be16(b, i + 2).ok_or("a truncated JPEG")?);
        let data = b.get(i + 4..i + 2 + len).ok_or("a truncated JPEG")?;
        match marker {
            0xe0 if data.starts_with(b"JFIF\0") && data.len() >= 12 => {
                jfif = Some((
                    data[7],
                    be16(data, 8).unwrap_or(1),
                    be16(data, 10).unwrap_or(1),
                ));
            }
            0xe1 if data.starts_with(b"Exif\0\0") => exif = orientation(&data[6..]),
            0xee if data.starts_with(b"Adobe") && data.len() >= 12 => adobe = Some(data[11]),
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                let h = be16(data, 1).ok_or("a truncated JPEG")?;
                let w = be16(data, 3).ok_or("a truncated JPEG")?;
                frame = Some((w, h, *data.get(5).ok_or("a truncated JPEG")?));
                break;
            }
            0xda | 0xd9 => break,
            _ => {}
        }
        i += 2 + len;
    }
    let (w, h, comps) = frame.ok_or("a JPEG without a frame")?;
    if w == 0 || h == 0 || !matches!(comps, 1 | 3 | 4) {
        return Err(format!("a JPEG of {w}×{h} with {comps} components"));
    }
    if u64::from(w) * u64::from(h) > MAX_STREAM_PIXELS {
        return Err(format!("a JPEG of {w}×{h} pixels"));
    }

    let mut out = vec![0xff, 0xd8];
    let segment = |out: &mut Vec<u8>, marker: u8, body: &[u8]| {
        out.extend_from_slice(&[0xff, marker]);
        out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(body);
    };
    if let Some((units, xd, yd)) = jfif {
        let mut body = b"JFIF\0\x01\x01".to_vec();
        body.push(units);
        body.extend_from_slice(&xd.to_be_bytes());
        body.extend_from_slice(&yd.to_be_bytes());
        body.extend_from_slice(&[0, 0]); // no thumbnail
        segment(&mut out, 0xe0, &body);
    }
    if let Some(o) = exif {
        let mut body = b"Exif\0\0".to_vec();
        body.extend(orientation_tiff(o));
        segment(&mut out, 0xe1, &body);
    }
    if comps == 4 || adobe.is_some() {
        // version 100, flags 0, transform: none for four components.
        let transform = adobe.filter(|_| comps != 4).unwrap_or(0);
        let mut body = b"Adobe\0\x64\0\0\0\0\0".to_vec();
        body[11] = transform;
        segment(&mut out, 0xee, &body);
    }
    // One quantization table of ones, a Huffman table with one code each
    // for "DC difference 0" and "end of block".
    let mut dqt = vec![0u8];
    dqt.extend_from_slice(&[1; 64]);
    segment(&mut out, 0xdb, &dqt);
    let mut sof = vec![8];
    sof.extend_from_slice(&h.to_be_bytes());
    sof.extend_from_slice(&w.to_be_bytes());
    sof.push(comps);
    for id in 1..=comps {
        sof.extend_from_slice(&[id, 0x11, 0]);
    }
    segment(&mut out, 0xc0, &sof);
    for class in [0x00u8, 0x10] {
        let mut dht = vec![class, 1];
        dht.extend_from_slice(&[0; 15]);
        dht.push(0);
        segment(&mut out, 0xc4, &dht);
    }
    let mut sos = vec![comps];
    for id in 1..=comps {
        sos.extend_from_slice(&[id, 0x00]);
    }
    sos.extend_from_slice(&[0, 63, 0]);
    segment(&mut out, 0xda, &sos);
    // Every block is two zero bits: code "0" for the DC difference 0 and
    // for the end of the block. The last byte is padded with ones.
    let blocks = u64::from(w).div_ceil(8) * u64::from(h).div_ceil(8) * u64::from(comps);
    let bits = blocks * 2;
    let mut data = vec![0u8; bits.div_ceil(8) as usize];
    if bits % 8 != 0
        && let Some(last) = data.last_mut()
    {
        *last = 0xffu8 >> (bits % 8);
    }
    out.extend(data);
    out.extend_from_slice(&[0xff, 0xd9]);
    Ok(out)
}

// ------------------------------------------------------------------- GIF

fn gif(b: &[u8]) -> Result<Vec<u8>, String> {
    let (w, h) = (
        le16(b, 6).ok_or("short GIF")?,
        le16(b, 8).ok_or("short GIF")?,
    );
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("a GIF of {w}×{h} pixels"));
    }
    let mut out = Vec::new();
    let mut encoder = ::gif::Encoder::new(&mut out, w, h, &[GREY, GREY, GREY, 0, 0, 0])
        .map_err(|e| e.to_string())?;
    let frame = ::gif::Frame {
        width: w,
        height: h,
        buffer: Cow::Owned(vec![0; usize::from(w) * usize::from(h)]),
        ..::gif::Frame::default()
    };
    encoder.write_frame(&frame).map_err(|e| e.to_string())?;
    encoder.into_inner().map_err(|e| e.to_string())?;
    Ok(out)
}

// ------------------------------------------------------------------ WebP

fn webp(b: &[u8]) -> Result<Vec<u8>, String> {
    let le24 = |i: usize| {
        b.get(i..i + 3)
            .map(|s| u32::from(s[0]) | u32::from(s[1]) << 8 | u32::from(s[2]) << 16)
    };
    let (w, h, alpha) = match b.get(12..16) {
        Some(b"VP8X") => (
            le24(24).ok_or("short WebP")? + 1,
            le24(27).ok_or("short WebP")? + 1,
            b.get(20).is_some_and(|f| f & 0x10 != 0),
        ),
        Some(b"VP8L") => {
            let bits = b.get(21..25).ok_or("short WebP")?;
            let v = u32::from_le_bytes([bits[0], bits[1], bits[2], bits[3]]);
            ((v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1, v >> 28 & 1 == 1)
        }
        Some(b"VP8 ") => (
            u32::from(le16(b, 26).ok_or("short WebP")? & 0x3fff),
            u32::from(le16(b, 28).ok_or("short WebP")? & 0x3fff),
            false,
        ),
        _ => return Err("a WebP of a kind we cannot read".into()),
    };
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("a WebP of {w}×{h} pixels"));
    }
    // The orientation sits in an EXIF chunk of the RIFF container.
    let mut exif = None;
    let mut i = 12;
    while let (Some(kind), Some(len)) = (b.get(i..i + 4), b.get(i + 4..i + 8)) {
        let len = u32::from_le_bytes([len[0], len[1], len[2], len[3]]) as usize;
        if kind == b"EXIF"
            && let Some(d) = b.get(i + 8..i + 8 + len)
        {
            exif = orientation(d.strip_prefix(b"Exif\0\0").unwrap_or(d));
        }
        i = i.saturating_add(8 + len + (len & 1));
    }

    let (color, channels) = if alpha {
        (image_webp::ColorType::Rgba8, 4)
    } else {
        (image_webp::ColorType::Rgb8, 3)
    };
    let mut out = Vec::new();
    let mut encoder = image_webp::WebPEncoder::new(&mut out);
    if let Some(o) = exif {
        encoder.set_exif_metadata(orientation_tiff(o));
    }
    let pixel: &[u8] = if alpha {
        &[GREY, GREY, GREY, 0xff]
    } else {
        &[GREY; 3]
    };
    let pixels = pixel.repeat(w as usize * h as usize);
    debug_assert_eq!(pixels.len(), w as usize * h as usize * channels);
    encoder
        .encode(&pixels, w, h, color)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

// ------------------------------------------------------------------- SVG

fn svg(b: &[u8]) -> Result<Vec<u8>, String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    let text = std::str::from_utf8(b).map_err(|_| "an SVG that is not UTF-8")?;
    let mut reader = Reader::from_str(text);
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref().ends_with("svg") => {
                let mut out = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\"");
                for a in e.attributes().with_checks(false).flatten() {
                    let key = a.key.as_ref().to_string();
                    if matches!(
                        key.as_str(),
                        "width" | "height" | "viewBox" | "preserveAspectRatio"
                    ) {
                        let v = a.value.as_ref();
                        // Already escaped as written.
                        out.push_str(&format!(" {key}=\"{}\"", v.replace('"', "&quot;")));
                    }
                }
                out.push_str("><rect width=\"100%\" height=\"100%\" fill=\"#ccc\"/></svg>\n");
                return Ok(out.into_bytes());
            }
            Event::Eof => return Err("an SVG without a root element".into()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use setzer_model::asset::Asset;

    /// What the importer reads of a picture.
    fn read(b: &[u8]) -> (String, Option<u32>, Option<u32>, Option<i32>) {
        let a = Asset::new("x", b.to_vec());
        (
            a.media_type,
            a.width_px,
            a.height_px,
            a.dpi.map(|d| d.round() as i32),
        )
    }

    fn png_with(w: u32, h: u32, color: ::png::ColorType, depth: ::png::BitDepth) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = ::png::Encoder::new(&mut out, w, h);
        enc.set_color(color);
        enc.set_depth(depth);
        if color == ::png::ColorType::Indexed {
            enc.set_palette(vec![10, 20, 30, 200, 100, 50]);
        }
        enc.set_pixel_dims(Some(::png::PixelDimensions {
            xppu: 11811,
            yppu: 11811,
            unit: ::png::Unit::Meter,
        }));
        let mut wr = enc.write_header().unwrap();
        let row = (w as usize * color.samples() * depth as usize).div_ceil(8);
        wr.write_image_data(&vec![0x5a; row * h as usize]).unwrap();
        wr.finish().unwrap();
        out
    }

    #[test]
    fn a_png_keeps_size_dpi_and_colour_mode() {
        use ::png::{BitDepth::*, ColorType::*};
        for (color, depth) in [
            (Grayscale, Eight),
            (Grayscale, One),
            (Rgb, Eight),
            (Rgb, Sixteen),
            (Rgba, Eight),
            (GrayscaleAlpha, Eight),
            (Indexed, Four),
            (Indexed, Eight),
        ] {
            let src = png_with(37, 23, color, depth);
            let blank = blank(&src).unwrap();
            assert_eq!(read(&blank), read(&src), "{color:?} {depth:?}");
            assert_eq!(read(&blank).3, Some(300));
            // The header says the same colour type and depth.
            assert_eq!(&blank[24..26], &src[24..26]);
            assert!(blank.len() < src.len() + 200);
            // And it decodes, to one colour all over.
            let mut dec = ::png::Decoder::new(std::io::Cursor::new(&blank))
                .read_info()
                .unwrap();
            let mut buf = vec![0; dec.output_buffer_size().unwrap()];
            let info = dec.next_frame(&mut buf).unwrap();
            assert_eq!((info.width, info.height), (37, 23));
            if color == Indexed {
                // Every index lies inside the palette.
                let entries = dec.info().palette.as_ref().map_or(0, |p| p.len() / 3);
                assert!(entries > 0 && buf.iter().all(|&i| usize::from(i) < entries));
            }
            let px = (buf.len() / (37 * 23)).max(1);
            assert!(
                buf.chunks(px).all(|c| c == &buf[..c.len()]),
                "{color:?} {depth:?}"
            );
        }
    }

    /// A damaged header (its CRC is not checked) of `w` × 1, RGBA at 16 bits.
    fn wide_header(w: u32) -> Vec<u8> {
        let mut src = b"\x89PNG\r\n\x1a\n".to_vec();
        src.extend_from_slice(&13u32.to_be_bytes());
        src.extend_from_slice(b"IHDR");
        src.extend_from_slice(&w.to_be_bytes());
        src.extend_from_slice(&1u32.to_be_bytes());
        src.extend_from_slice(&[16, 6, 0, 0, 0]);
        src.extend_from_slice(&[0; 4]);
        src.extend_from_slice(&0u32.to_be_bytes());
        src.extend_from_slice(b"IEND");
        src.extend_from_slice(&[0; 4]);
        src
    }

    #[test]
    fn a_wide_row_within_the_limit_is_written() {
        // 1,000,000 × 1 RGBA at 16 bits: an 8 MB row.
        let out = blank(&wide_header(1_000_000)).unwrap();
        assert!(out.len() < 200_000, "{}", out.len());
        assert_eq!(read(&out).1, Some(1_000_000));
    }

    #[test]
    fn a_header_that_claims_a_row_past_the_limit_is_an_error() {
        // 50,000,000 × 1 RGBA at 16 bits is a 400 MB row; the encoder would
        // hold three of them. Refused before it is asked.
        for w in [3_000_000, 50_000_000] {
            assert!(
                matches!(blank(&wide_header(w)), Err(e) if e.contains("row")),
                "{w}"
            );
        }
        // Past the pixel cap as well.
        assert!(blank(&wide_header(u32::MAX)).is_err());
    }

    #[test]
    fn a_png_keeps_its_rotation_tag() {
        let mut info = ::png::Info::with_size(5, 4);
        info.exif_metadata = Some(Cow::Owned(orientation_tiff(6)));
        let mut src = Vec::new();
        let mut wr = ::png::Encoder::with_info(&mut src, info)
            .unwrap()
            .write_header()
            .unwrap();
        wr.write_image_data(&[0; 20]).unwrap();
        drop(wr);
        let blank = blank(&src).unwrap();
        let dec = ::png::Decoder::new(std::io::Cursor::new(&blank))
            .read_info()
            .unwrap();
        let exif = dec.info().exif_metadata.as_ref().unwrap();
        assert_eq!(orientation(exif), Some(6));
    }

    /// A tiny JPEG with the segments the importer reads.
    fn jpeg_src(w: u16, h: u16, comps: u8, dpi: u16, orientation: Option<u16>) -> Vec<u8> {
        let mut b = vec![0xff, 0xd8, 0xff, 0xe0, 0, 16];
        b.extend_from_slice(b"JFIF\0\x01\x01\x01");
        b.extend_from_slice(&dpi.to_be_bytes());
        b.extend_from_slice(&dpi.to_be_bytes());
        b.extend_from_slice(&[0, 0]);
        if let Some(o) = orientation {
            let mut body = b"Exif\0\0".to_vec();
            body.extend(orientation_tiff(o));
            b.extend_from_slice(&[0xff, 0xe1]);
            b.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
            b.extend(body);
        }
        b.extend_from_slice(&[0xff, 0xc2, 0, 8 + 3 * comps, 8]);
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.push(comps);
        for id in 1..=comps {
            b.extend_from_slice(&[id, 0x22, 0]);
        }
        b.extend_from_slice(&[0xff, 0xda, 0, 2, 1, 2, 3, 4, 5]);
        b
    }

    #[test]
    fn a_jpeg_keeps_size_dpi_components_and_rotation() {
        for comps in [1u8, 3, 4] {
            let src = jpeg_src(641, 479, comps, 300, Some(8));
            let blank = blank(&src).unwrap();
            assert_eq!(read(&blank), read(&src));
            assert_eq!(
                read(&blank),
                ("image/jpeg".into(), Some(641), Some(479), Some(300))
            );
            // Components in the frame header, rotation in Exif.
            let sof = blank.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
            assert_eq!(blank[sof + 9], comps);
            let exif = blank.windows(6).position(|w| w == b"Exif\0\0").unwrap();
            assert_eq!(orientation(&blank[exif + 6..]), Some(8));
            assert_eq!(comps == 4, blank.windows(5).any(|w| w == b"Adobe"));
            assert!(blank.ends_with(&[0xff, 0xd9]));
            // 81 × 60 blocks × comps, two bits each.
            let blocks = 81 * 60 * usize::from(comps);
            assert!(blank.len() < 400 + blocks.div_ceil(4));
        }
        assert!(blank(&jpeg_src(1, 1, 3, 72, None)).is_ok());
    }

    #[test]
    fn a_gif_keeps_its_screen() {
        let mut src = Vec::new();
        let mut enc = ::gif::Encoder::new(&mut src, 19, 11, &[0, 0, 0, 9, 9, 9]).unwrap();
        let frame = ::gif::Frame {
            width: 19,
            height: 11,
            buffer: Cow::Owned(vec![1; 19 * 11]),
            ..::gif::Frame::default()
        };
        enc.write_frame(&frame).unwrap();
        enc.into_inner().unwrap();
        let blank = blank(&src).unwrap();
        assert_eq!(read(&blank), read(&src));
        assert_eq!(read(&blank).1, Some(19));
        let mut opts = ::gif::DecodeOptions::new();
        opts.set_color_output(::gif::ColorOutput::Indexed);
        let mut dec = opts.read_info(std::io::Cursor::new(&blank)).unwrap();
        let f = dec.read_next_frame().unwrap().unwrap();
        assert!(f.buffer.iter().all(|&b| b == 0));
    }

    #[test]
    fn a_webp_keeps_its_size_alpha_and_rotation() {
        for alpha in [false, true] {
            let mut src = Vec::new();
            let mut enc = image_webp::WebPEncoder::new(&mut src);
            enc.set_exif_metadata(orientation_tiff(3));
            let (color, n) = if alpha {
                (image_webp::ColorType::Rgba8, 4)
            } else {
                (image_webp::ColorType::Rgb8, 3)
            };
            enc.encode(&vec![7; 17 * 9 * n], 17, 9, color).unwrap();
            let blank = blank(&src).unwrap();
            assert_eq!(read(&blank), read(&src));
            assert_eq!(read(&blank).1, Some(17));
            let mut dec = image_webp::WebPDecoder::new(std::io::Cursor::new(&blank)).unwrap();
            assert_eq!(dec.has_alpha(), alpha);
            assert_eq!(dec.dimensions(), (17, 9));
            assert_eq!(orientation(&dec.exif_metadata().unwrap().unwrap()), Some(3));
            let mut buf = vec![0; dec.output_buffer_size().unwrap()];
            dec.read_image(&mut buf).unwrap();
            let px = if alpha { 4 } else { 3 };
            assert!(
                buf.chunks(px)
                    .all(|c| c[..3] == [GREY; 3] && (!alpha || c[3] == 0xff))
            );
        }
    }

    #[test]
    fn an_svg_keeps_its_size_and_nothing_else() {
        let src = br##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="120" height="80" viewBox="0 0 120 80" id="x"><text>Ein Landarzt</text></svg>"##;
        let blank = blank(src).unwrap();
        let text = String::from_utf8(blank.clone()).unwrap();
        assert!(text.contains("width=\"120\"") && text.contains("viewBox=\"0 0 120 80\""));
        assert!(!text.contains("Landarzt") && !text.contains("id=") && text.contains("<rect"));
        assert_eq!(read(&blank).0, "image/svg+xml");
    }

    #[test]
    fn other_formats_keep_their_header_and_length() {
        let mut bmp = b"BM".to_vec();
        bmp.extend((0..300u16).map(|n| (n % 251) as u8 | 1));
        let blank = blank(&bmp).unwrap();
        assert_eq!(blank.len(), bmp.len());
        assert_eq!(&blank[..128], &bmp[..128]);
        assert!(blank[128..].iter().all(|&b| b == 0));
        assert_eq!(ImageFormat::sniff(&blank), ImageFormat::Unsupported("BMP"));
    }

    #[test]
    fn damaged_pictures_are_errors() {
        assert!(blank(&[0x89, b'P', b'N', b'G', 1, 2, 3]).is_err());
        assert!(blank(&[0xff, 0xd8, 0xff, 0xe0, 0, 99]).is_err());
        assert!(blank(b"GIF89a").is_err());
        assert!(blank(b"RIFF\0\0\0\0WEBPVP8X").is_err());
    }
}
