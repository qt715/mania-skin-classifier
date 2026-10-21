//! skin.ini parsing mirroring osu!lazer's LegacyDecoder / LegacyManiaSkinDecoder line rules
//! (see `skinini.py`): repeated, case-sensitive `[Mania]` sections; lines before `Keys:`
//! belong to that section; the first section for a key count wins.

use std::collections::{BTreeMap, HashMap};

pub const VALID_KEY_COUNTS: [i64; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 16, 18];

pub struct ManiaSection {
    pub keys: usize,
    pub values: HashMap<String, String>,
}

pub struct SkinIni {
    pub general: HashMap<String, String>,
    pub mania: BTreeMap<usize, ManiaSection>,
    pub warnings: Vec<String>,
}

pub fn decode_text(data: &[u8]) -> (String, Vec<String>) {
    let body = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    match std::str::from_utf8(body) {
        Ok(s) => (s.to_owned(), vec![]),
        Err(_) => (
            String::from_utf8_lossy(body).into_owned(),
            vec!["skin.ini is not valid UTF-8; undecodable bytes replaced".to_owned()],
        ),
    }
}

/// Python `str.splitlines()` semantics, so exotic line breaks split identically.
fn split_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let brk = matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if brk {
            out.push(&text[start..i]);
            let mut end = i + c.len_utf8();
            if c == '\r'
                && let Some(&(j, '\n')) = it.peek()
            {
                it.next();
                end = j + 1;
            }
            start = end;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

fn split_key_val(line: &str) -> (String, String) {
    match line.split_once(':') {
        Some((k, v)) => (k.trim().to_owned(), v.trim().to_owned()),
        None => (line.trim().to_owned(), String::new()),
    }
}

pub fn parse_skin_ini(data: &[u8]) -> SkinIni {
    let (text, mut warnings) = decode_text(data);
    let mut general = HashMap::new();
    let mut mania: BTreeMap<usize, ManiaSection> = BTreeMap::new();
    let mut section: Option<String> = None;
    let mut pending: Vec<String> = Vec::new();
    // The section currently receiving lines; `duplicate` sections are parsed but discarded.
    let mut current: Option<ManiaSection> = None;
    let mut current_is_dup = false;

    let flush =
        |cur: &mut Option<ManiaSection>, dup: bool, mania: &mut BTreeMap<usize, ManiaSection>| {
            if let Some(sec) = cur.take()
                && !dup
            {
                mania.insert(sec.keys, sec);
            }
        };

    for (n, raw) in split_lines(&text).into_iter().enumerate() {
        let lineno = n + 1;
        if raw.trim().is_empty() || raw.trim_start().starts_with("//") {
            continue;
        }
        let line = match raw.find("//") {
            Some(idx) if idx > 0 => &raw[..idx],
            _ => raw,
        }
        .trim_end();
        if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
            flush(&mut current, current_is_dup, &mut mania);
            section = Some(line[1..line.len() - 1].to_owned());
            pending.clear();
            current_is_dup = false;
            continue;
        }
        match section.as_deref() {
            Some("General") => {
                let (k, v) = split_key_val(line);
                general.insert(k, v);
            }
            Some("Mania") => {
                let (k, v) = split_key_val(line);
                if k == "Keys" {
                    flush(&mut current, current_is_dup, &mut mania);
                    let keys = match v.parse::<i64>() {
                        Ok(k) => k,
                        Err(_) => {
                            warnings.push(format!(
                                "line {lineno}: unparseable Keys value {}; section ignored",
                                py_repr(&v)
                            ));
                            pending.clear();
                            current_is_dup = false;
                            continue;
                        }
                    };
                    if !VALID_KEY_COUNTS.contains(&keys) {
                        warnings.push(format!(
                            "line {lineno}: Keys: {keys} is not a valid key count; section ignored"
                        ));
                        pending.clear();
                        current_is_dup = false;
                        continue;
                    }
                    let keys = keys as usize;
                    current_is_dup = mania.contains_key(&keys);
                    if current_is_dup {
                        warnings.push(format!("line {lineno}: duplicate [Mania] section for Keys: {keys}; ignored (first wins)"));
                    }
                    let mut sec = ManiaSection {
                        keys,
                        values: HashMap::new(),
                    };
                    for p in pending.drain(..) {
                        let (pk, pv) = split_key_val(&p);
                        if !pk.is_empty() {
                            sec.values.insert(pk, pv);
                        }
                    }
                    current = Some(sec);
                } else if let Some(sec) = current.as_mut() {
                    if !k.is_empty() {
                        sec.values.insert(k, v);
                    }
                } else {
                    pending.push(line.to_owned());
                }
            }
            _ => {}
        }
    }
    flush(&mut current, current_is_dup, &mut mania);
    SkinIni {
        general,
        mania,
        warnings,
    }
}

/// Python `repr()` of a str, for warning text parity.
fn py_repr(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_sections_pending_lines_and_first_wins() {
        let ini = parse_skin_ini(b"[Mania]\nNoteBodyStyle: 2\nKeys: 4\nNoteImage0: a\n[Mania]\nKeys: 4\nNoteImage0: b\n[Mania]\nKeys: 7\n");
        assert_eq!(ini.mania.keys().copied().collect::<Vec<_>>(), vec![4, 7]);
        assert_eq!(ini.mania[&4].values["NoteImage0"], "a");
        assert_eq!(ini.mania[&4].values["NoteBodyStyle"], "2");
        assert!(ini.warnings.iter().any(|w| w.contains("duplicate")));
    }

    #[test]
    fn case_sensitive_names_invalid_counts_and_comments() {
        let ini = parse_skin_ini("\u{feff}[mania]\nKeys: 4\n[Mania]\nKeys: 11\n[Mania]\n// Keys: 5\nKeys: 6 // six\nnoteimage0: x\nNoteImage0: y\n".as_bytes());
        assert_eq!(ini.mania.keys().copied().collect::<Vec<_>>(), vec![6]);
        assert_eq!(ini.mania[&6].values["NoteImage0"], "y");
        assert_eq!(ini.warnings.len(), 1);
    }
}
