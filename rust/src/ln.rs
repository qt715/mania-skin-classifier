//! Cut amount of an LN body: background rows above where the body starts (Percy "d").
//! Port of `ln.py`; see there for the three kinds of background.

use crate::config::Thresholds;
use crate::imageio::Rgba;

pub struct Cut {
    pub top_blank_px: f64,
    pub body_start_row: usize,
    pub empty: bool,
    pub ghost_alpha: Option<f64>,
    pub cover_band_rows: Option<usize>,
}

fn row(top: &Rgba, y: usize) -> &[u8] {
    &top.data[y * top.width * 4..(y + 1) * top.width * 4]
}

/// Visible pixels of a row and whether they all lie within `tol` of `colour` (RGBA).
fn row_matches(r: &[u8], blank: i64, colour: &[f64; 4], tol: i64) -> (f64, bool) {
    let mut vis = 0usize;
    let mut maxd = 0f64;
    for p in r.as_chunks::<4>().0 {
        if p[3] as i64 > blank {
            vis += 1;
            for c in 0..4 {
                maxd = maxd.max((p[c] as f64 - colour[c]).abs());
            }
        }
    }
    let frac = vis as f64 / (r.len() / 4).max(1) as f64;
    (frac, vis > 0 && maxd <= tol as f64)
}

/// Number of leading rows forming an opaque, uniform cover band unlike the deep body.
fn uniform_band(top: &Rgba, t: &Thresholds) -> usize {
    if top.height < 2 || top.width == 0 {
        return 0;
    }
    let first = row(top, 0);
    let vis: Vec<&[u8; 4]> = first
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] as i64 > t.blank_alpha)
        .collect();
    if (vis.len() as f64 / top.width as f64) < 0.9 {
        return 0;
    }
    let mut colour = [0f64; 4];
    for c in 0..4 {
        colour[c] = crate::shapes::median(vis.iter().map(|p| p[c] as f64).collect());
    }
    if !row_matches(first, t.blank_alpha, &colour, t.band_colour_tol).1 {
        return 0;
    }
    let (deep_frac, deep_same) = row_matches(
        row(top, top.height - 1),
        t.blank_alpha,
        &colour,
        t.band_colour_tol,
    );
    if deep_frac >= 0.9 && deep_same {
        return 0; // the "band" is just a uniform body
    }
    (0..top.height)
        .take_while(|&y| {
            let (frac, same) = row_matches(row(top, y), t.blank_alpha, &colour, t.band_colour_tol);
            frac >= 0.9 && same
        })
        .count()
}

/// numpy.percentile(..., linear interpolation).
fn percentile(v: &[u8], q: f64) -> f64 {
    let mut s: Vec<u8> = v.to_vec();
    s.sort_unstable();
    let pos = q / 100.0 * (s.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    s[lo] as f64 + (s[hi] as f64 - s[lo] as f64) * (pos - lo as f64)
}

pub fn cut_amount(rows: &[u8], top: &Rgba, scale: u32, t: &Thresholds) -> Cut {
    let h = rows.len();
    if h == 0 {
        return Cut {
            top_blank_px: 0.0,
            body_start_row: 0,
            empty: true,
            ghost_alpha: None,
            cover_band_rows: None,
        };
    }
    let lower = if h >= 2 { &rows[h / 2..] } else { rows };
    let reference = percentile(lower, 90.0);
    let ghost = (t.blank_alpha as f64).max(t.ghost_alpha_rel * reference);
    let band = uniform_band(top, t);
    let bg = |i: usize| i < band || (rows[i] as f64) <= ghost;

    let min_run = ((t.body_min_run_px * scale as f64).ceil() as usize).max(1);
    let mut start = None;
    let mut i = 0;
    while i < h {
        if bg(i) {
            i += 1;
            continue;
        }
        let s = i;
        while i < h && !bg(i) {
            i += 1;
        }
        // The first long run, or one reaching the bottom, is the body.
        if i - s >= min_run || i == h {
            start = Some(s);
            break;
        }
    }
    let (ghost_alpha, cover_band_rows) = (Some(crate::shapes::pyround(ghost, 1)), Some(band));
    match start {
        None => Cut {
            top_blank_px: h as f64 / scale as f64,
            body_start_row: h,
            empty: true,
            ghost_alpha,
            cover_band_rows,
        },
        Some(s) => Cut {
            top_blank_px: crate::shapes::pyround(s as f64 / scale as f64, 1),
            body_start_row: s,
            empty: false,
            ghost_alpha,
            cover_band_rows,
        },
    }
}
