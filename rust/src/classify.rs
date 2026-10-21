//! Per-skin pipeline (port of `classify.py`): parse skin.ini, resolve per-column sprites,
//! classify, aggregate per key mode. Output JSON matches the Python reference key for key.

use crate::assets::{Asset, fallback_index, resolve_column, resolve_tail};
use crate::config::Thresholds;
use crate::imageio::{Rgba, body_rows, header, load_rgba};
use crate::ln::cut_amount;
use crate::shapes::{SHAPES, classify_mask, opaque_mask, pyround};
use crate::skinini::parse_skin_ini;
use crate::sources::SkinSource;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

pub const SCHEMA_VERSION: u32 = 1;
pub const TOOL_NAME: &str = "mania-skin-classifier";
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

struct Profile {
    width: u64,
    height: u64,
    rows: Vec<u8>,
    top: Rgba,
}

/// Results keyed by file content hash: identical images are decoded and analysed once.
#[derive(Default)]
struct Cache {
    shapes: HashMap<String, Map<String, Value>>,
    profiles: HashMap<String, Result<std::rc::Rc<Profile>, String>>,
}

fn obj(pairs: Vec<(&str, Value)>) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

fn note_shape(
    src: &SkinSource,
    asset: &Asset,
    t: &Thresholds,
    cache: &mut Cache,
) -> Result<Map<String, Value>, String> {
    let path = &asset.first().path;
    let digest = src.sha256(path)?;
    if !cache.shapes.contains_key(&digest) {
        let entry = match src.read(path).and_then(|d| load_rgba(&d)) {
            Ok((rgba, stride)) => {
                let (mask, kind) = opaque_mask(&rgba, t);
                let r = classify_mask(&mask, rgba.width, rgba.height, t);
                obj(vec![
                    ("shape", json!(r.shape)),
                    ("confidence", json!(r.confidence)),
                    ("reason", json!(r.reason)),
                    ("mask", json!(kind)),
                    (
                        "size",
                        json!([rgba.width as u64 * stride, rgba.height as u64 * stride]),
                    ),
                    ("features", Value::Object(r.features)),
                ])
            }
            Err(e) => obj(vec![
                ("shape", json!("unknown")),
                ("confidence", json!(0.0)),
                ("reason", json!(e)),
            ]),
        };
        cache.shapes.insert(digest.clone(), entry);
    }
    let mut out = obj(vec![("sha256", json!(digest))]);
    out.extend(cache.shapes[&digest].clone());
    Ok(out)
}

fn profile(
    src: &SkinSource,
    path: &str,
    t: &Thresholds,
    cache: &mut Cache,
) -> Result<(String, Result<std::rc::Rc<Profile>, String>), String> {
    let digest = src.sha256(path)?;
    let p = cache
        .profiles
        .entry(digest.clone())
        .or_insert_with(|| {
            let data = src.read(path)?;
            let hd = header(&data)?;
            let (rows, top) = body_rows(&data, t.top_scan_rows)?;
            Ok(std::rc::Rc::new(Profile {
                width: hd.width,
                height: hd.height,
                rows,
                top,
            }))
        })
        .clone();
    Ok((digest, p))
}

/// Python `int(str)`: optional sign, digits, surrounding whitespace, `_` between digits.
fn int_style(raw: Option<&String>) -> Option<i64> {
    let s = raw?.trim();
    if s.is_empty() || s.starts_with('_') || s.ends_with('_') || s.contains("__") {
        return None;
    }
    s.replace('_', "").parse().ok()
}

fn ln_column(
    src: &SkinSource,
    values: &HashMap<String, String>,
    keys: usize,
    col: usize,
    t: &Thresholds,
    cache: &mut Cache,
    unresolved: &mut Vec<Value>,
) -> Result<Map<String, Value>, String> {
    let (style, style_from) = match (
        int_style(values.get(&format!("NoteBodyStyle{col}"))),
        int_style(values.get("NoteBodyStyle")),
    ) {
        (Some(s), _) => (s, format!("NoteBodyStyle{col}")),
        (None, Some(s)) => (s, "NoteBodyStyle".to_owned()),
        _ => (1, "default".to_owned()),
    };
    let (body_name, body) = resolve_column(src, values, keys, col, "L");
    let mut ev = obj(vec![
        ("body_style", json!(style)),
        ("body_style_from", json!(style_from)),
        ("body_lookup", json!(body_name)),
    ]);
    let with = |head: Vec<(&str, Value)>, ev: Map<String, Value>| {
        let mut m = obj(head);
        m.extend(ev);
        m
    };
    let Some(body) = body else {
        unresolved.push(json!({"key_mode": keys, "column": col, "role": "ln_body", "lookup": body_name, "configured": values.contains_key(&format!("NoteImage{col}L"))}));
        return Ok(with(
            vec![
                ("cut", json!("unknown")),
                ("reason", json!("LN body unresolved")),
            ],
            ev,
        ));
    };
    let (digest, prof) = profile(src, &body.first().path, t, cache)?;
    ev.insert(
        "body_file".into(),
        json!(src.original_path(&body.first().path)),
    );
    ev.insert("body_sha256".into(), json!(digest));
    ev.insert("body_frames".into(), json!(body.frames.len()));
    let prof = match prof {
        Ok(p) => p,
        Err(e) => {
            return Ok(with(
                vec![("cut", json!("unknown")), ("reason", json!(e))],
                ev,
            ));
        }
    };
    let scale = body.first().scale;
    let logical_h = prof.height as f64 / scale as f64;
    let cut = cut_amount(&prof.rows, &prof.top, scale, t);
    let mut analysis = obj(vec![
        ("body_start_row", json!(cut.body_start_row)),
        ("empty", json!(cut.empty)),
    ]);
    if let (Some(g), Some(b)) = (cut.ghost_alpha, cut.cover_band_rows) {
        analysis.insert("ghost_alpha".into(), json!(g));
        analysis.insert("cover_band_rows".into(), json!(b));
    }
    ev.insert("body_size".into(), json!([prof.width, prof.height]));
    ev.insert("body_scale".into(), json!(scale));
    ev.insert("body_logical_height".into(), json!(pyround(logical_h, 1)));
    ev.insert("body_top_blank_px".into(), json!(cut.top_blank_px));
    ev.insert("body_cut_analysis".into(), Value::Object(analysis));

    let (tail_role, tail_name, tail) = resolve_tail(src, values, keys, col);
    let mut tail_transparent = None;
    if let Some(tail) = tail {
        let (_, tprof) = profile(src, &tail.first().path, t, cache)?;
        if let Ok(tp) = tprof {
            tail_transparent = Some(
                tp.rows
                    .iter()
                    .copied()
                    .max()
                    .map(|m| m as i64 <= t.tail_transparent_alpha)
                    .unwrap_or(true),
            );
        }
        ev.insert("tail_role".into(), json!(tail_role));
        ev.insert(
            "tail_file".into(),
            json!(src.original_path(&tail.first().path)),
        );
        ev.insert("tail_transparent".into(), json!(tail_transparent));
    } else {
        ev.insert("tail_role".into(), Value::Null);
        ev.insert("tail_lookup".into(), json!(tail_name));
        ev.insert("tail_transparent".into(), Value::Null);
    }

    if cut.empty {
        return Ok(with(
            vec![
                ("cut", json!("no")),
                ("reason", json!("LN body has no visible content")),
            ],
            ev,
        ));
    }
    let checks = [
        ("style", style == t.percy_body_style),
        ("tall", logical_h >= t.percy_min_height),
        ("top_blank", cut.top_blank_px >= t.percy_min_top_blank_px),
    ];
    ev.insert(
        "checks".into(),
        Value::Object(
            checks
                .iter()
                .map(|(k, v)| ((*k).to_owned(), json!(v)))
                .collect(),
        ),
    );
    if checks.iter().all(|c| c.1) {
        let tt = tail_transparent == Some(true);
        let conf = if tt { 0.95 } else { 0.75 };
        let why = format!(
            "tall body with blank top{}",
            if tt {
                ", transparent tail"
            } else {
                ", tail not transparent"
            }
        );
        return Ok(with(
            vec![
                ("cut", json!("yes")),
                ("confidence", json!(conf)),
                ("reason", json!(why)),
            ],
            ev,
        ));
    }
    let failed: Vec<&str> = checks.iter().filter(|c| !c.1).map(|c| c.0).collect();
    Ok(with(
        vec![
            ("cut", json!("no")),
            ("reason", json!(format!("fails: {}", failed.join(", ")))),
        ],
        ev,
    ))
}

fn aggregate_cut(cols: &[String]) -> &'static str {
    let has = |s: &str| cols.iter().any(|c| c == s);
    if has("yes") && has("no") {
        "mixed"
    } else if has("unknown") {
        "unknown"
    } else if has("yes") {
        "all"
    } else {
        "none"
    }
}

/// Classify one skin. `Err` means the skin could not be read at all (I/O or archive errors);
/// problems with individual sprites are reported inside the result instead.
pub fn classify_skin(
    src: &SkinSource,
    t: &Thresholds,
    details: bool,
    content_hash: bool,
) -> Result<Map<String, Value>, String> {
    let mut skin = obj(vec![
        ("input", json!(src.location)),
        ("kind", json!(src.kind)),
        ("name", json!(src.name)),
    ]);
    if content_hash {
        skin.insert("content_hash".into(), json!(src.content_hash()?));
    }
    let mut key_modes = Map::new();
    let mut unresolved: Vec<Value> = Vec::new();
    let mut warnings: Vec<Value> = Vec::new();
    let mut detail_modes = Map::new();

    if !src.has("skin.ini") {
        warnings.push(json!("no skin.ini; no declared mania key modes"));
    } else {
        let ini = parse_skin_ini(&src.read("skin.ini")?);
        warnings.extend(ini.warnings.iter().map(|w| json!(w)));
        if let Some(n) = ini.general.get("Name").filter(|n| !n.is_empty()) {
            skin.insert("name".into(), json!(n));
        }
        skin.insert("ini_version".into(), json!(ini.general.get("Version")));

        let mut cache = Cache::default();
        for (&keys, sec) in &ini.mania {
            let values = &sec.values;
            let mut cols_detail = Vec::new();
            let mut shapes: Vec<&str> = Vec::new();
            let mut unclassified: Vec<usize> = Vec::new();
            let mut cuts: Vec<String> = Vec::new();
            for col in 0..keys {
                let (note_name, note) = resolve_column(src, values, keys, col, "");
                let mut cd = obj(vec![
                    ("column", json!(col)),
                    ("fallback", json!(fallback_index(keys, col))),
                    ("note_lookup", json!(note_name)),
                ]);
                match note {
                    None => {
                        unresolved.push(json!({"key_mode": keys, "column": col, "role": "note", "lookup": note_name, "configured": values.contains_key(&format!("NoteImage{col}"))}));
                        cd.insert("note".into(), json!({"shape": "unresolved"}));
                        unclassified.push(col);
                    }
                    Some(note) => {
                        let ns = note_shape(src, &note, t, &mut cache)?;
                        let shape = ns["shape"].as_str().unwrap().to_owned();
                        let mut n = obj(vec![
                            ("file", json!(src.original_path(&note.first().path))),
                            ("scale", json!(note.first().scale)),
                            ("frames", json!(note.frames.len())),
                        ]);
                        n.extend(ns);
                        cd.insert("note".into(), Value::Object(n));
                        match SHAPES.iter().find(|s| **s == shape) {
                            Some(s) => shapes.push(s),
                            None => unclassified.push(col),
                        }
                    }
                }
                let ln = ln_column(src, values, keys, col, t, &mut cache, &mut unresolved)?;
                cuts.push(ln["cut"].as_str().unwrap().to_owned());
                cd.insert("ln".into(), Value::Object(ln));
                cols_detail.push(Value::Object(cd));
            }
            let ordered: Vec<&str> = SHAPES
                .iter()
                .copied()
                .filter(|s| shapes.contains(s))
                .collect();
            key_modes.insert(
                keys.to_string(),
                json!({
                    "note_shapes": ordered,
                    "ln_cut": aggregate_cut(&cuts),
                    "unclassified_columns": unclassified,
                }),
            );
            detail_modes.insert(keys.to_string(), Value::Array(cols_detail));
        }
    }

    let mut out = obj(vec![
        ("schema_version", json!(SCHEMA_VERSION)),
        ("tool", json!({"name": TOOL_NAME, "version": TOOL_VERSION})),
        ("skin", Value::Object(skin)),
        ("key_modes", Value::Object(key_modes)),
        ("unresolved", Value::Array(unresolved)),
        ("warnings", Value::Array(warnings)),
    ]);
    if details {
        out.insert("thresholds".into(), serde_json::to_value(t).unwrap());
        out.insert("details".into(), Value::Object(detail_modes));
    }
    Ok(out)
}
