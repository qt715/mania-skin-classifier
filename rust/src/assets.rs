//! Texture name resolution matching osu!lazer's legacy skin lookups (see `assets.py` for the
//! lazer source references).

use crate::sources::{SkinSource, normalize};
use std::collections::HashMap;

const IMAGE_EXTENSIONS: [&str; 3] = [".png", ".jpg", ".jpeg"];

#[derive(Clone, Debug)]
pub struct Frame {
    pub path: String,
    pub scale: u32,
}

#[derive(Clone, Debug)]
pub struct Asset {
    pub frames: Vec<Frame>,
}

impl Asset {
    pub fn first(&self) -> &Frame {
        &self.frames[0]
    }
}

pub fn is_special_column(keys: usize, column: usize) -> bool {
    // SpecialStyle (even key counts) is deliberately not applied: lazer's texture fallback
    // ignores it, and stable's exact even-key layout is not publicly documented.
    keys % 2 == 1 && column == keys / 2
}

pub fn fallback_index(keys: usize, column: usize) -> &'static str {
    if is_special_column(keys, column) {
        return "S";
    }
    let distance_to_edge = column.min(keys - 1 - column);
    if distance_to_edge.is_multiple_of(2) {
        "1"
    } else {
        "2"
    }
}

/// Splits `stem.ext` when `.ext` is an image extension (Python `rpartition('.')` semantics).
fn image_ext_split(norm: &str) -> Option<(&str, &str)> {
    let (stem, ext) = norm.rsplit_once('.')?;
    let dotted = &norm[stem.len()..];
    IMAGE_EXTENSIONS.contains(&dotted).then_some((stem, ext))
}

fn file_candidates(name: &str) -> Vec<String> {
    let base = normalize(name);
    if image_ext_split(&base).is_some() {
        return vec![base];
    }
    let mut v = vec![base.clone()];
    v.extend(IMAGE_EXTENSIONS.iter().map(|e| format!("{base}{e}")));
    v
}

fn texture(source: &SkinSource, name: &str) -> Option<Frame> {
    let name = name.replace("@2x", "");
    let norm = normalize(&name);
    let two_x = match image_ext_split(&norm) {
        Some((stem, ext)) => format!("{stem}@2x.{ext}"),
        None => format!("{norm}@2x"),
    };
    for cand in file_candidates(&two_x) {
        if source.has(&cand) {
            return Some(Frame {
                path: cand,
                scale: 2,
            });
        }
    }
    for cand in file_candidates(&norm) {
        if source.has(&cand) {
            return Some(Frame {
                path: cand,
                scale: 1,
            });
        }
    }
    None
}

pub fn resolve_texture(source: &SkinSource, name: &str) -> Option<Asset> {
    if let Some(first) = texture(source, &format!("{name}-0")) {
        let mut frames = vec![first];
        let mut i = 1;
        while let Some(f) = texture(source, &format!("{name}-{i}")) {
            frames.push(f);
            i += 1;
        }
        return Some(Asset { frames });
    }
    texture(source, name).map(|f| Asset { frames: vec![f] })
}

/// Texture name for `NoteImage{column}{suffix}` or its default.
pub fn column_lookup(
    values: &HashMap<String, String>,
    keys: usize,
    column: usize,
    suffix: &str,
) -> String {
    match values.get(&format!("NoteImage{column}{suffix}")) {
        Some(v) if !v.is_empty() => v.clone(),
        _ => format!("mania-note{}{suffix}", fallback_index(keys, column)),
    }
}

pub fn resolve_column(
    source: &SkinSource,
    values: &HashMap<String, String>,
    keys: usize,
    column: usize,
    suffix: &str,
) -> (String, Option<Asset>) {
    let name = column_lookup(values, keys, column, suffix);
    let asset = resolve_texture(source, &name);
    (name, asset)
}

/// Tail texture with lazer's T -> H -> note fallback: (role used, lookup, asset).
pub fn resolve_tail(
    source: &SkinSource,
    values: &HashMap<String, String>,
    keys: usize,
    column: usize,
) -> (&'static str, String, Option<Asset>) {
    let mut first_name = String::new();
    for (role, suffix) in [("tail", "T"), ("head", "H"), ("note", "")] {
        let (name, asset) = resolve_column(source, values, keys, column, suffix);
        if asset.is_some() {
            return (role, name, asset);
        }
        if first_name.is_empty() {
            first_name = name;
        }
    }
    ("tail", first_name, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_matches_lazer() {
        let l = |k: usize| {
            (0..k)
                .map(|c| fallback_index(k, c))
                .collect::<Vec<_>>()
                .join("")
        };
        assert_eq!(l(4), "1221");
        assert_eq!(l(7), "121S121");
        assert_eq!(l(10), "1212112121");
        assert_eq!(l(1), "S");
    }
}
