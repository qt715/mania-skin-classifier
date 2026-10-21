//! Bounded-memory image access (see `imageio.py`). PNG LN bodies are always streamed row by
//! row, keeping only per-row maximum alpha plus the first `top_rows` rows.

use image::{ImageFormat, ImageReader};
use std::io::Cursor;

pub const STREAM_PIXELS: u64 = 16_000_000;
pub const MASK_MAX_SIDE: u64 = 1024;

/// Row-major RGBA8 pixels.
#[derive(Clone)]
pub struct Rgba {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Rgba {
    pub fn px(&self, x: usize, y: usize) -> &[u8] {
        let i = (y * self.width + x) * 4;
        &self.data[i..i + 4]
    }
}

pub struct Header {
    pub width: u64,
    pub height: u64,
    pub format: Option<ImageFormat>,
}

pub fn header(data: &[u8]) -> Result<Header, String> {
    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| format!("unreadable image: {e}"))?;
    let format = reader.format();
    if format.is_none() {
        return Err("unreadable image: unknown format".into());
    }
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| format!("unreadable image: {e}"))?;
    Ok(Header {
        width: w as u64,
        height: h as u64,
        format,
    })
}

fn decode_full(data: &[u8]) -> Result<Rgba, String> {
    let mut reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| format!("decode failed: {e}"))?;
    reader.no_limits(); // dimensions are gated by the caller via `header`
    let img = reader
        .decode()
        .map_err(|e| format!("decode failed: {e}"))?
        .into_rgba8();
    let (w, h) = img.dimensions();
    Ok(Rgba {
        width: w as usize,
        height: h as usize,
        data: img.into_raw(),
    })
}

struct PngRows<'a> {
    reader: png::Reader<Cursor<&'a [u8]>>,
    color: png::ColorType,
    width: usize,
    height: usize,
}

impl<'a> PngRows<'a> {
    fn new(data: &'a [u8]) -> Result<Option<Self>, String> {
        let mut dec =
            png::Decoder::new_with_limits(Cursor::new(data), png::Limits { bytes: usize::MAX });
        dec.set_transformations(png::Transformations::normalize_to_color8());
        let reader = dec
            .read_info()
            .map_err(|e| format!("png decode failed: {e}"))?;
        if reader.info().interlaced {
            return Ok(None);
        }
        let (color, _) = reader.output_color_type();
        let (w, h) = (reader.info().width as usize, reader.info().height as usize);
        Ok(Some(Self {
            reader,
            color,
            width: w,
            height: h,
        }))
    }

    /// Calls `f(row_index, rgba_row)` for every row.
    fn for_each(mut self, mut f: impl FnMut(usize, &[u8])) -> Result<(), String> {
        let mut rgba = vec![0u8; self.width * 4];
        let mut i = 0;
        while let Some(row) = self
            .reader
            .next_row()
            .map_err(|e| format!("png decode failed: {e}"))?
        {
            to_rgba(self.color, row.data(), &mut rgba);
            f(i, &rgba);
            i += 1;
        }
        Ok(())
    }
}

fn to_rgba(color: png::ColorType, src: &[u8], dst: &mut [u8]) {
    use png::ColorType::*;
    let n = dst.len() / 4;
    match color {
        Rgba => dst.copy_from_slice(&src[..n * 4]),
        Rgb => (0..n).for_each(|i| {
            dst[i * 4..i * 4 + 3].copy_from_slice(&src[i * 3..i * 3 + 3]);
            dst[i * 4 + 3] = 255;
        }),
        GrayscaleAlpha => (0..n).for_each(|i| {
            let g = src[i * 2];
            dst[i * 4..i * 4 + 4].copy_from_slice(&[g, g, g, src[i * 2 + 1]]);
        }),
        Grayscale | Indexed => (0..n).for_each(|i| {
            let g = src[i];
            dst[i * 4..i * 4 + 4].copy_from_slice(&[g, g, g, 255]);
        }),
    }
}

/// Per-row maximum alpha for every row plus the first `top_rows` rows as RGBA.
pub fn body_rows(data: &[u8], top_rows: usize) -> Result<(Vec<u8>, Rgba), String> {
    let hd = header(data)?;
    if hd.format == Some(ImageFormat::Png)
        && let Some(rows) = PngRows::new(data)?
    {
        let (w, h) = (rows.width, rows.height);
        let mut out = Vec::with_capacity(h);
        let keep = top_rows.min(h);
        let mut top = Vec::with_capacity(w * 4 * keep);
        rows.for_each(|i, r| {
            out.push(r.iter().skip(3).step_by(4).copied().max().unwrap_or(0));
            if i < keep {
                top.extend_from_slice(r);
            }
        })?;
        let n = top.len() / (w * 4).max(1);
        return Ok((
            out,
            Rgba {
                width: w,
                height: n,
                data: top,
            },
        ));
    }
    let full = decode_full(data)?;
    let w = full.width;
    let maxes = full
        .data
        .chunks(w * 4)
        .map(|r| r.iter().skip(3).step_by(4).copied().max().unwrap_or(0))
        .collect();
    let keep = top_rows.min(full.height);
    let top = Rgba {
        width: w,
        height: keep,
        data: full.data[..w * 4 * keep].to_vec(),
    };
    Ok((maxes, top))
}

/// RGBA pixels plus the integer subsample stride applied (1 = full resolution).
pub fn load_rgba(data: &[u8]) -> Result<(Rgba, u64), String> {
    let hd = header(data)?;
    if hd.width * hd.height <= STREAM_PIXELS || hd.format != Some(ImageFormat::Png) {
        return Ok((decode_full(data)?, 1));
    }
    let stride = hd.width.max(hd.height).div_ceil(MASK_MAX_SIDE) as usize;
    let Some(rows) = PngRows::new(data)? else {
        return Ok((decode_full(data)?, 1));
    };
    let w = rows.width.div_ceil(stride);
    let mut kept = Vec::new();
    let mut n = 0;
    rows.for_each(|i, r| {
        if i % stride == 0 {
            for x in (0..rows_width(r)).step_by(stride) {
                kept.extend_from_slice(&r[x * 4..x * 4 + 4]);
            }
            n += 1;
        }
    })?;
    Ok((
        Rgba {
            width: w,
            height: n,
            data: kept,
        },
        stride as u64,
    ))
}

fn rows_width(r: &[u8]) -> usize {
    r.len() / 4
}
