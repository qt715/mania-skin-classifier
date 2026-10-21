//! Note sprite shape classification (port of `shapes.py`).
//!
//! The Python reference rasterizes templates at 512 px with OpenCV, rotates and crops them,
//! then area-resizes to the sample. Here each template is analytic geometry normalized to
//! its own bounding box and rendered straight at the sample size with 4x4 supersampling
//! (coverage >= 1/2, the same cut as area-resize + `> 127`). Rendered templates are cached
//! per sample size, which a bulk run hits constantly (most note sprites share a few sizes).

use crate::config::Thresholds;
use crate::imageio::Rgba;
use crate::raster::{self, Gray};
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::LazyLock;

pub const SHAPES: [&str; 4] = ["circle", "arrow", "bar", "etc"];
const WORK: f64 = 160.0;
const BASE_RES: f64 = 511.0; // (TEMPLATE_RES - 1) in the reference
const SUPERSAMPLE: usize = 4;
const ROTATIONS: [i32; 8] = [0, 45, 90, 135, 180, 225, 270, 315];
/// etc templates that are unchanged by a 180-degree turn; arrows never are.
const POINT_SYMMETRIC: [&str; 8] = [
    "diamond",
    "square",
    "round_square",
    "hexagon_flat",
    "hexagon_pointy",
    "octagon",
    "plus",
    "star4",
];

pub struct ShapeResult {
    pub shape: &'static str,
    pub confidence: f64,
    pub reason: String,
    pub features: Map<String, Value>,
}

/// Python `round(x, n)`.
pub fn pyround(x: f64, n: usize) -> f64 {
    format!("{x:.n$}").parse().unwrap()
}

#[derive(Clone, Copy, PartialEq)]
enum Gate {
    Any,
    Round,
    Elongated,
    Compact,
}

enum Geom {
    /// Polygon in coordinates normalized to the template's bounding box.
    Poly(Vec<(f64, f64)>),
    Ellipse,
}

struct Template {
    cls: &'static str,
    name: String,
    gate: Gate,
    geom: Geom,
}

fn star(n: usize, inner: f64) -> Vec<(f64, f64)> {
    (0..2 * n)
        .map(|i| {
            let r = if i % 2 == 0 { 0.5 } else { 0.5 * inner };
            let a = -PI / 2.0 + i as f64 * PI / n as f64;
            (0.5 + r * a.cos(), 0.5 + r * a.sin())
        })
        .collect()
}

fn regular(n: usize, phase: f64) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let a = phase + 2.0 * PI * i as f64 / n as f64;
            (0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin())
        })
        .collect()
}

fn heart() -> Vec<(f64, f64)> {
    let n = 400;
    let pts: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let t = 2.0 * PI * i as f64 / (n - 1) as f64;
            (
                16.0 * t.sin().powi(3),
                -(13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos()),
            )
        })
        .collect();
    normalize_bbox(pts)
}

fn normalize_bbox(pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in &pts {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    pts.into_iter()
        .map(|(x, y)| ((x - x0) / (x1 - x0), (y - y0) / (y1 - y0)))
        .collect()
}

/// Rotate like cv2.getRotationMatrix2D(angle) + warpAffine (positive = counter-clockwise on
/// screen), then re-fit to the bounding box.
fn rotated(pts: &[(f64, f64)], angle: i32) -> Vec<(f64, f64)> {
    let (b, a) = (angle as f64).to_radians().sin_cos();
    normalize_bbox(
        pts.iter()
            .map(|&(x, y)| {
                let (x, y) = (x * BASE_RES, y * BASE_RES);
                (a * x + b * y, -b * x + a * y)
            })
            .collect(),
    )
}

static FIXED_TEMPLATES: LazyLock<Vec<Template>> = LazyLock::new(build_fixed_templates);

fn build_fixed_templates() -> Vec<Template> {
    let mut arrows: Vec<(String, Vec<(f64, f64)>)> = Vec::new();
    for hh in [0.4, 0.55, 0.7] {
        for sw in [0.3, 0.45, 0.6] {
            arrows.push((
                format!("stem_arrow_h{hh}_s{sw}"),
                vec![
                    (0.5, 0.0),
                    (1.0, hh),
                    (0.5 + sw / 2.0, hh),
                    (0.5 + sw / 2.0, 1.0),
                    (0.5 - sw / 2.0, 1.0),
                    (0.5 - sw / 2.0, hh),
                    (0.0, hh),
                ],
            ));
        }
    }
    arrows.push(("triangle".into(), vec![(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)]));
    for th in [0.2, 0.35, 0.5] {
        arrows.push((
            format!("chevron_t{th}"),
            vec![
                (0.5, 0.0),
                (1.0, 0.5),
                (1.0, 0.5 + th),
                (0.5, th),
                (0.0, 0.5 + th),
                (0.0, 0.5),
            ],
        ));
    }
    for n in [0.25, 0.45] {
        arrows.push((
            format!("dart_n{n}"),
            vec![(0.5, 0.0), (1.0, 1.0), (0.5, 1.0 - n), (0.0, 1.0)],
        ));
    }
    // DDR/StepMania arrows: a 90-degree head whose arms end in square cuts, plus a stem.
    for c in [0.12, 0.2, 0.28] {
        for sw in [0.3, 0.45] {
            let y = 2.0 * c + sw / 2.0;
            arrows.push((
                format!("ddr_arrow_c{c}_s{sw}"),
                vec![
                    (0.5, 0.0),
                    (1.0, 0.5),
                    (1.0 - c, 0.5 + c),
                    (0.5 + sw / 2.0, y),
                    (0.5 + sw / 2.0, 1.0),
                    (0.5 - sw / 2.0, 1.0),
                    (0.5 - sw / 2.0, y),
                    (c, 0.5 + c),
                    (0.0, 0.5),
                ],
            ));
        }
    }
    let etc: Vec<(&str, Vec<(f64, f64)>)> = vec![
        (
            "diamond",
            vec![(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)],
        ),
        ("star5", star(5, 0.45)),
        ("star4", star(4, 0.4)),
        (
            "plus",
            vec![
                (0.33, 0.0),
                (0.67, 0.0),
                (0.67, 0.33),
                (1.0, 0.33),
                (1.0, 0.67),
                (0.67, 0.67),
                (0.67, 1.0),
                (0.33, 1.0),
                (0.33, 0.67),
                (0.0, 0.67),
                (0.0, 0.33),
                (0.33, 0.33),
            ],
        ),
        ("hexagon_flat", regular(6, 0.0)),
        ("hexagon_pointy", regular(6, PI / 2.0)),
        ("octagon", regular(8, PI / 8.0)),
    ];
    let mut out = Vec::new();
    for (name, pts) in &arrows {
        for r in ROTATIONS {
            out.push(Template {
                cls: "arrow",
                name: format!("{name}@{r}"),
                gate: Gate::Any,
                geom: Geom::Poly(rotated(pts, r)),
            });
        }
    }
    out.push(Template {
        cls: "circle",
        name: "ellipse".into(),
        gate: Gate::Round,
        geom: Geom::Ellipse,
    });
    for (name, pts) in &etc {
        let gate = if ["diamond", "star5", "star4", "plus"].contains(name) {
            Gate::Any
        } else {
            Gate::Compact
        };
        let rots: &[i32] = if ["star4", "plus"].contains(name) {
            &[0, 45]
        } else {
            &[0]
        };
        for &r in rots {
            out.push(Template {
                cls: "etc",
                name: format!("{name}@{r}"),
                gate,
                geom: Geom::Poly(rotated(pts, r)),
            });
        }
    }
    out.push(Template {
        cls: "etc",
        name: "heart".into(),
        gate: Gate::Compact,
        geom: Geom::Poly(heart()),
    });
    out
}

fn render_fixed(geom: &Geom, w: usize, h: usize) -> Gray {
    match geom {
        Geom::Poly(pts) => {
            let px: Vec<(f64, f64)> = pts
                .iter()
                .map(|&(u, v)| (u * w as f64 - 0.5, v * h as f64 - 0.5))
                .collect();
            raster::fill_polygon(w, h, &px, SUPERSAMPLE)
        }
        Geom::Ellipse => {
            let mut g = Gray::new(w, h);
            let ss = SUPERSAMPLE;
            let need = (ss * ss).div_ceil(2);
            for y in 0..h {
                for x in 0..w {
                    let mut c = 0;
                    for sy in 0..ss {
                        let v = (y as f64 + (sy as f64 + 0.5) / ss as f64) / h as f64 - 0.5;
                        for sx in 0..ss {
                            let u = (x as f64 + (sx as f64 + 0.5) / ss as f64) / w as f64 - 0.5;
                            if u * u + v * v <= 0.25 {
                                c += 1;
                            }
                        }
                    }
                    if c >= need {
                        g.data[y * w + x] = 255;
                    }
                }
            }
            g
        }
    }
}

/// cv2.rectangle fills plus anti-aliased corner circles (centre within r + 0.5 after `> 127`).
fn round_rect(w: usize, h: usize, radius: f64) -> Gray {
    let mut g = Gray::new(w, h);
    let r = radius
        .min(w as f64 / 2.0)
        .min(h as f64 / 2.0)
        .round_ties_even() as isize;
    if r <= 0 {
        g.data.fill(255);
        return g;
    }
    let (wi, hi) = (w as isize, h as isize);
    let centres = [
        (r, r),
        (wi - 1 - r, r),
        (r, hi - 1 - r),
        (wi - 1 - r, hi - 1 - r),
    ];
    let lim = (r as f64 + 0.5) * (r as f64 + 0.5);
    for y in 0..hi {
        for x in 0..wi {
            let in_rect = (x >= r && x <= wi - 1 - r) || (y >= r && y <= hi - 1 - r);
            let in_circle = centres
                .iter()
                .any(|&(cx, cy)| ((x - cx).pow(2) + (y - cy).pow(2)) as f64 <= lim);
            if in_rect || in_circle {
                g.data[(y * wi + x) as usize] = 255;
            }
        }
    }
    g
}

fn aspect_templates(w: usize, h: usize) -> Vec<(&'static str, String, Gate, Gray)> {
    let short = w.min(h) as f64;
    let mut out = vec![
        (
            "bar",
            "rect".to_owned(),
            Gate::Elongated,
            round_rect(w, h, 0.0),
        ),
        (
            "bar",
            "round_rect_0.2".to_owned(),
            Gate::Elongated,
            round_rect(w, h, 0.2 * short),
        ),
        (
            "bar",
            "stadium".to_owned(),
            Gate::Elongated,
            round_rect(w, h, 0.5 * short),
        ),
        (
            "etc",
            "square".to_owned(),
            Gate::Compact,
            round_rect(w, h, 0.0),
        ),
        (
            "etc",
            "round_square".to_owned(),
            Gate::Compact,
            round_rect(w, h, 0.2 * short),
        ),
    ];
    let (wf, hf) = (w as f64 - 1.0, h as f64 - 1.0);
    for (name, inset) in [("chamfer_bar", 0.3), ("pointed_bar", 0.5)] {
        let c = inset * short;
        let pts = if w >= h {
            vec![
                (c, 0.0),
                (wf - c, 0.0),
                (wf, c),
                (wf, hf - c),
                (wf - c, hf),
                (c, hf),
                (0.0, hf - c),
                (0.0, c),
            ]
        } else {
            vec![
                (0.0, c),
                (c, 0.0),
                (wf - c, 0.0),
                (wf, c),
                (wf, hf - c),
                (wf - c, hf),
                (c, hf),
                (0.0, hf - c),
            ]
        };
        out.push((
            "bar",
            name.to_owned(),
            Gate::Elongated,
            raster::fill_polygon_dilated(w, h, &pts, 0.5),
        ));
    }
    out
}

/// Opaque silhouette of an RGBA sprite and how it was derived.
pub fn opaque_mask(img: &Rgba, t: &Thresholds) -> (Vec<bool>, &'static str) {
    let n = img.width * img.height;
    let alpha = || img.data.iter().skip(3).step_by(4).copied();
    let amax = alpha().max().unwrap_or(0) as i64;
    if amax == 0 {
        return (vec![false; n], "transparent");
    }
    if (alpha().min().unwrap_or(0) as i64) < 250 {
        let thr = (t.alpha_floor as f64).max(t.alpha_rel * amax as f64);
        return (alpha().map(|a| a as f64 >= thr).collect(), "alpha");
    }
    // Fully opaque image (e.g. JPEG): separate the sprite from a uniform border colour.
    let (w, h) = (img.width, img.height);
    let mut border: Vec<[i64; 3]> = Vec::with_capacity(2 * (w + h));
    let rgb = |x: usize, y: usize| {
        let p = img.px(x, y);
        [p[0] as i64, p[1] as i64, p[2] as i64]
    };
    (0..w).for_each(|x| border.push(rgb(x, 0)));
    (0..w).for_each(|x| border.push(rgb(x, h - 1)));
    (0..h).for_each(|y| border.push(rgb(0, y)));
    (0..h).for_each(|y| border.push(rgb(w - 1, y)));
    let bg: Vec<f64> = (0..3)
        .map(|c| median(border.iter().map(|p| p[c] as f64).collect()))
        .collect();
    let dist = |p: [i64; 3]| {
        (0..3)
            .map(|c| (p[c] as f64 - bg[c]).abs())
            .fold(0.0, f64::max)
    };
    let tol = t.opaque_bg_tolerance as f64;
    let ok = border.iter().filter(|&&p| dist(p) <= tol).count() as f64 / border.len() as f64;
    if ok < 0.9 {
        return (vec![true; n], "opaque-rect");
    }
    let fg: Vec<bool> = (0..n)
        .map(|i| {
            dist([
                img.data[i * 4] as i64,
                img.data[i * 4 + 1] as i64,
                img.data[i * 4 + 2] as i64,
            ]) > tol
        })
        .collect();
    if (fg.iter().filter(|&&v| v).count() as f64) / (n as f64) < 0.01 {
        return (vec![true; n], "opaque-rect");
    }
    (fg, "opaque-bg")
}

pub fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn silhouette(
    mask: &[bool],
    w: usize,
    h: usize,
    t: &Thresholds,
) -> Option<(Gray, Map<String, Value>)> {
    let full = Gray {
        w,
        h,
        data: mask.iter().map(|&b| if b { 255 } else { 0 }).collect(),
    };
    let (x0, y0, x1, y1) = full.bbox()?;
    let m = full.crop(x0, y0, x1, y1);
    let scale = WORK / m.w.max(m.h) as f64;
    let nw = ((m.w as f64 * scale).round_ties_even() as usize).max(1);
    let nh = ((m.h as f64 * scale).round_ties_even() as usize).max(1);
    let mut m = if scale < 1.0 {
        raster::resize_area(&m, nw, nh)
    } else {
        raster::resize_linear(&m, nw, nh)
    };
    m.threshold(127);
    // Drop specks (sparkles, debug labels) before closing so they cannot fuse onto the shape.
    let (labels, areas) = raster::components(&m);
    if areas.len() > 2 {
        let max = *areas[1..].iter().max().unwrap() as f64;
        for (p, &l) in labels.iter().enumerate() {
            if l != 0 && (areas[l as usize] as f64) < t.component_min_frac * max {
                m.data[p] = 0;
            }
        }
    }
    let k = ((t.close_frac * WORK).round_ties_even() as usize).max(1);
    let m = raster::close(&m.pad(k), k);
    let (labels, areas) = raster::components(&m);
    if areas.len() <= 1 {
        return None;
    }
    let largest = *areas[1..].iter().max().unwrap();
    let keep: Vec<bool> = areas
        .iter()
        .enumerate()
        .map(|(i, &a)| i > 0 && a as f64 >= t.component_min_frac * largest as f64)
        .collect();
    let sig = Gray {
        w: m.w,
        h: m.h,
        data: labels
            .iter()
            .map(|&l| if keep[l as usize] { 255 } else { 0 })
            .collect(),
    };
    let filled = raster::fill_holes(&sig);
    let (x0, y0, x1, y1) = filled.bbox()?;
    let total: usize = areas
        .iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(a, _)| a)
        .sum();
    let mut feats = Map::new();
    feats.insert(
        "components".into(),
        json!(keep.iter().filter(|&&k| k).count()),
    );
    feats.insert(
        "largest_component_frac".into(),
        json!(pyround(largest as f64 / total as f64, 3)),
    );
    Some((filled.crop(x0, y0, x1, y1), feats))
}

fn gate_ok(gate: Gate, aspect: f64, t: &Thresholds) -> bool {
    let ls = aspect.max(1.0 / aspect);
    match gate {
        Gate::Round => ls <= t.circle_max_aspect,
        Gate::Elongated => ls >= t.bar_min_aspect,
        Gate::Compact => ls < t.bar_min_aspect,
        Gate::Any => true,
    }
}

fn solidity(sil: &Gray, area: usize) -> f64 {
    // Sum of per-component convex hull areas over pixel centres, like summing
    // cv2.contourArea(cv2.convexHull(c)) over external contours.
    let (labels, areas) = raster::components(sil);
    let mut per: Vec<Vec<(i64, i64)>> = vec![Vec::new(); areas.len()];
    for y in 0..sil.h {
        let mut first: HashMap<u32, usize> = HashMap::new();
        let mut last: HashMap<u32, usize> = HashMap::new();
        for x in 0..sil.w {
            let l = labels[y * sil.w + x];
            if l != 0 {
                first.entry(l).or_insert(x);
                last.insert(l, x);
            }
        }
        for (l, x) in first {
            per[l as usize].push((x as i64, y as i64));
            per[l as usize].push((last[&l] as i64, y as i64));
        }
    }
    let hull: f64 = per.into_iter().skip(1).map(raster::hull_area).sum();
    area as f64 / if hull == 0.0 { 1.0 } else { hull }
}

/// Rendered template bitsets per sample size, indexed like `FIXED_TEMPLATES`.
type RenderCache = HashMap<(usize, usize), Vec<Option<Vec<u64>>>>;

thread_local! {
    static RENDERED: RefCell<RenderCache> = RefCell::new(HashMap::new());
}

fn bits(g: &Gray) -> Vec<u64> {
    let mut out = vec![0u64; g.data.len().div_ceil(64)];
    for (i, &v) in g.data.iter().enumerate() {
        if v != 0 {
            out[i / 64] |= 1 << (i % 64);
        }
    }
    out
}

fn iou(a: &[u64], b: &[u64]) -> f64 {
    let (mut i, mut u) = (0u32, 0u32);
    for (x, y) in a.iter().zip(b) {
        i += (x & y).count_ones();
        u += (x | y).count_ones();
    }
    if u == 0 { 0.0 } else { i as f64 / u as f64 }
}

pub fn classify_mask(mask: &[bool], mw: usize, mh: usize, t: &Thresholds) -> ShapeResult {
    let unknown = |reason: &str, features| ShapeResult {
        shape: "unknown",
        confidence: 0.0,
        reason: reason.into(),
        features,
    };
    let Some((sil, mut feats)) = silhouette(mask, mw, mh, t) else {
        return unknown("empty sprite", Map::new());
    };
    let (w, h) = (sil.w, sil.h);
    if mw.min(mh) < 2 || w.max(h) < 8 {
        return unknown("sprite too small", feats);
    }
    let aspect = w as f64 / h as f64;
    let area = sil.count();
    let sb = bits(&sil);
    let rot = Gray {
        w,
        h,
        data: sil.data.iter().rev().copied().collect(),
    };
    let point_symmetry = iou(&sb, &bits(&rot));
    feats.insert("aspect".into(), json!(pyround(aspect, 3)));
    feats.insert(
        "extent".into(),
        json!(pyround(area as f64 / (w * h) as f64, 3)),
    );
    feats.insert("solidity".into(), json!(pyround(solidity(&sil, area), 3)));
    feats.insert("point_symmetry".into(), json!(pyround(point_symmetry, 3)));

    // (class, iou, template name) in first-seen class order, like the reference's dict.
    let mut best: Vec<(&'static str, f64, String)> = Vec::new();
    let mut consider =
        |cls: &'static str, name: &str, score: f64| match best.iter_mut().find(|b| b.0 == cls) {
            Some(b) if score > b.1 => {
                b.1 = score;
                b.2 = name.to_owned();
            }
            Some(_) => {}
            None => best.push((cls, score, name.to_owned())),
        };
    let fixed = &*FIXED_TEMPLATES;
    RENDERED.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() > 512 {
            cache.clear();
        }
        let slots = cache
            .entry((w, h))
            .or_insert_with(|| vec![None; fixed.len()]);
        for (i, tpl) in fixed.iter().enumerate() {
            if !gate_ok(tpl.gate, aspect, t)
                || (tpl.cls == "arrow" && point_symmetry >= t.arrow_max_point_symmetry)
            {
                continue;
            }
            let tb = slots[i].get_or_insert_with(|| bits(&render_fixed(&tpl.geom, w, h)));
            consider(tpl.cls, &tpl.name, iou(&sb, tb));
        }
    });
    for (cls, name, gate, img) in aspect_templates(w, h) {
        if gate_ok(gate, aspect, t) {
            consider(cls, &name, iou(&sb, &bits(&img)));
        }
    }

    // Stable sort: ties keep first-seen order, like Python's sorted().
    best.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    feats.insert(
        "iou".into(),
        Value::Object(
            best.iter()
                .map(|b| (b.0.to_owned(), json!(pyround(b.1, 3))))
                .collect(),
        ),
    );
    feats.insert(
        "best_template".into(),
        Value::Object(best.iter().map(|b| (b.0.to_owned(), json!(b.2))).collect()),
    );

    let arrow = best.iter().find(|b| b.0 == "arrow").cloned();
    if best[0].0 == "etc"
        && POINT_SYMMETRIC.contains(&best[0].2.split('@').next().unwrap())
        && point_symmetry < t.arrow_max_point_symmetry
        && arrow.as_ref().map(|a| a.1).unwrap_or(0.0) >= t.match_min_iou
    {
        // The silhouette breaks the template's 180-degree symmetry (e.g. a diamond-bodied
        // arrow with a stem stub), so the near-tied arrow match is the better explanation.
        let a = arrow.unwrap();
        best.retain(|b| b.0 != "arrow");
        best.insert(0, a);
    }
    let (top_cls, top_iou, top_name) = best[0].clone();
    let second = best.get(1).map(|b| b.1).unwrap_or(0.0);

    let lcf = feats["largest_component_frac"].as_f64().unwrap();
    if lcf < t.fragment_min_frac {
        return ShapeResult {
            shape: "etc",
            confidence: pyround(1.0 - lcf, 3),
            reason: "fragmented sprite (multiple separate parts)".into(),
            features: feats,
        };
    }
    if top_iou < t.match_min_iou {
        return ShapeResult {
            shape: "etc",
            confidence: pyround(1.0 - top_iou, 3),
            reason: format!("no template fits (best {top_cls} {top_iou:.2})"),
            features: feats,
        };
    }
    let margin = top_iou - second;
    if top_cls != "etc" && best.len() > 1 && best[1].0 != "etc" && margin < t.ambiguity_margin {
        let reason = format!("ambiguous between {top_cls} and {}", best[1].0);
        return ShapeResult {
            shape: "unknown",
            confidence: pyround(margin, 3),
            reason,
            features: feats,
        };
    }
    ShapeResult {
        shape: top_cls,
        confidence: pyround(top_iou.min(1.0), 3),
        reason: format!("matched {top_name}"),
        features: feats,
    }
}
