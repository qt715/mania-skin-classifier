//! One skin: an extracted folder or a single `.osk` archive, exposed through lazer's
//! case-insensitive, forward-slash path namespace.

use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn normalize(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches('/')
        .to_lowercase()
}

enum Backend {
    Dir {
        root: PathBuf,
    },
    /// `index` maps an entry's decoded name to its position in the central directory.
    Zip {
        archive: RefCell<zip::ZipArchive<File>>,
        index: HashMap<String, usize>,
    },
}

pub struct SkinSource {
    pub name: String,
    pub location: String,
    pub kind: &'static str,
    /// normalized path -> original (relative path or zip entry name)
    files: HashMap<String, String>,
    order: Vec<String>,
    backend: Backend,
    sha_cache: RefCell<HashMap<String, String>>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// If skin.ini is not at the root but exactly one copy exists in a subfolder, re-root there.
fn strip_root(files: Vec<(String, String)>) -> Vec<(String, String)> {
    if files.iter().any(|(n, _)| n == "skin.ini") {
        return files;
    }
    let candidates: Vec<&String> = files
        .iter()
        .map(|(n, _)| n)
        .filter(|n| n.rsplit('/').next() == Some("skin.ini"))
        .collect();
    if candidates.len() != 1 {
        return files;
    }
    let prefix = candidates[0][..candidates[0].len() - "skin.ini".len()].to_owned();
    files
        .into_iter()
        .filter_map(|(n, o)| n.strip_prefix(&prefix).map(|r| (r.to_owned(), o)))
        .collect()
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        // Like os.walk(followlinks=False): descend into real directories only, but list
        // symlinks to files as files.
        if ft.is_dir() {
            walk(root, &path, out)?;
        } else if path.metadata().map(|m| m.is_file()).unwrap_or(false) {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            out.push((normalize(&rel), rel));
        }
    }
    Ok(())
}

impl SkinSource {
    pub fn open(path: &Path) -> Result<Self, String> {
        if path.is_dir() {
            let mut list = Vec::new();
            walk(path, path, &mut list).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(Self::build(
                path,
                "folder",
                list,
                Backend::Dir {
                    root: path.to_path_buf(),
                },
            ))
        } else if path.is_file()
            && path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("osk") || e.eq_ignore_ascii_case("zip"))
                .unwrap_or(false)
        {
            let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut za = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut list = Vec::new();
            let mut index = HashMap::new();
            for i in 0..za.len() {
                let e = za
                    .by_index_raw(i)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                if !e.is_dir() {
                    let name = e
                        .name()
                        .map_err(|e| format!("{}: {e}", path.display()))?
                        .into_owned();
                    index.entry(name.clone()).or_insert(i);
                    list.push((normalize(&name), name));
                }
            }
            Ok(Self::build(
                path,
                "osk",
                list,
                Backend::Zip {
                    archive: RefCell::new(za),
                    index,
                },
            ))
        } else {
            Err(format!(
                "{} is neither a skin folder nor an .osk archive",
                path.display()
            ))
        }
    }

    fn build(
        path: &Path,
        kind: &'static str,
        list: Vec<(String, String)>,
        backend: Backend,
    ) -> Self {
        let mut files = HashMap::new();
        let mut order = Vec::new();
        for (n, o) in strip_root(list) {
            if let std::collections::hash_map::Entry::Vacant(e) = files.entry(n) {
                order.push(e.key().clone());
                e.insert(o);
            }
        }
        let name = if kind == "osk" {
            path.file_stem()
        } else {
            path.file_name()
        }
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
        Self {
            name,
            location: path.to_string_lossy().into_owned(),
            kind,
            files,
            order,
            backend,
            sha_cache: RefCell::default(),
        }
    }

    pub fn has(&self, norm: &str) -> bool {
        self.files.contains_key(norm)
    }

    pub fn paths(&self) -> &[String] {
        &self.order
    }

    pub fn original_path(&self, norm: &str) -> &str {
        &self.files[norm]
    }

    pub fn read(&self, norm: &str) -> Result<Vec<u8>, String> {
        let orig = &self.files[norm];
        match &self.backend {
            Backend::Dir { root } => {
                std::fs::read(root.join(orig)).map_err(|e| format!("{orig}: {e}"))
            }
            Backend::Zip { archive, index } => {
                let mut za = archive.borrow_mut();
                let mut e = za
                    .by_index(index[orig])
                    .map_err(|e| format!("{orig}: {e}"))?;
                let mut buf = Vec::with_capacity(e.size() as usize);
                e.read_to_end(&mut buf)
                    .map_err(|e| format!("{orig}: {e}"))?;
                Ok(buf)
            }
        }
    }

    pub fn sha256(&self, norm: &str) -> Result<String, String> {
        if let Some(h) = self.sha_cache.borrow().get(norm) {
            return Ok(h.clone());
        }
        let h = sha256_hex(&self.read(norm)?);
        self.sha_cache
            .borrow_mut()
            .insert(norm.to_owned(), h.clone());
        Ok(h)
    }

    /// Source-independent identity: the same skin as a folder or .osk hashes equally.
    pub fn content_hash(&self) -> Result<String, String> {
        let mut paths: Vec<&String> = self.order.iter().collect();
        paths.sort();
        let mut h = Sha256::new();
        for p in paths {
            h.update(p.as_bytes());
            h.update(b"\0");
            h.update(self.sha256(p)?.as_bytes());
            h.update(b"\n");
        }
        Ok(hex(&h.finalize()))
    }
}
