//! Bulk mode: classify many skins in parallel into JSONL (+ optional flat CSV), resumable.

use crate::classify::{SCHEMA_VERSION, TOOL_NAME, TOOL_VERSION, classify_skin};
use crate::config::Thresholds;
use crate::sources::SkinSource;
use rayon::prelude::*;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, IsTerminal, Read, Seek, SeekFrom, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct Job {
    pub id: String,
    pub path: PathBuf,
}

pub struct ScanOptions {
    pub output: PathBuf,
    pub csv: Option<PathBuf>,
    pub threads: usize,
    pub resume: bool,
    pub details: bool,
    pub content_hash: bool,
    pub thresholds: Thresholds,
}

fn is_skin_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten().any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("skin.ini")
                    && e.path().is_file()
            })
        })
        .unwrap_or(false)
}

/// Every `.osk` file and every folder with a `skin.ini` under `root` (skin folders are not
/// descended into, so variant subfolders inside a skin are not separate skins).
pub fn discover(root: &Path) -> std::io::Result<Vec<Job>> {
    let mut jobs = Vec::new();
    if is_skin_dir(root) {
        let id = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into());
        return Ok(vec![Job {
            id,
            path: root.to_path_buf(),
        }]);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let rel = path.strip_prefix(root).unwrap();
            if path.is_dir() {
                if is_skin_dir(&path) {
                    jobs.push(Job {
                        id: rel.to_string_lossy().into_owned(),
                        path,
                    });
                } else {
                    stack.push(path);
                }
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("osk"))
            {
                jobs.push(Job {
                    id: rel.with_extension("").to_string_lossy().into_owned(),
                    path,
                });
            }
        }
    }
    jobs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(jobs)
}

/// TSV `id<TAB>path` lines; blank lines and `#` comments are skipped.
pub fn read_list(file: &Path) -> Result<Vec<Job>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|(n, l)| match l.split_once('\t') {
            Some((id, p)) => Ok(Job {
                id: id.to_owned(),
                path: PathBuf::from(p),
            }),
            None => Err(format!(
                "{}:{}: expected id<TAB>path",
                file.display(),
                n + 1
            )),
        })
        .collect()
}

fn done_ids(output: &Path) -> HashSet<String> {
    let Ok(f) = File::open(output) else {
        return HashSet::new();
    };
    BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<Value>(&l).ok())
        .filter(|v| v.get("error").is_some_and(Value::is_null))
        .filter_map(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

/// Open for appending, terminating a final line cut short by an interrupted run.
fn open_append(path: &Path) -> std::io::Result<(File, bool)> {
    let mut f = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(path)?;
    let len = f.metadata()?.len();
    if len > 0 {
        f.seek(SeekFrom::End(-1))?;
        let mut last = [0u8; 1];
        f.read_exact(&mut last)?;
        if last[0] != b'\n' {
            f.write_all(b"\n")?;
        }
    }
    Ok((f, len == 0))
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

fn csv_rows(id: &str, record: &Map<String, Value>) -> String {
    let mut out = String::new();
    let Some(Value::Object(modes)) = record.get("key_modes") else {
        return out;
    };
    for (k, m) in modes {
        let join = |key: &str| {
            m[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string())
                })
                .collect::<Vec<_>>()
                .join("|")
        };
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            csv_field(id),
            k,
            join("note_shapes"),
            m["ln_cut"].as_str().unwrap(),
            join("unclassified_columns")
        ));
    }
    out
}

fn classify_job(job: &Job, opts: &ScanOptions) -> Map<String, Value> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let src = SkinSource::open(&job.path)?;
        classify_skin(&src, &opts.thresholds, opts.details, opts.content_hash)
    }));
    let mut rec = Map::new();
    rec.insert("id".into(), json!(job.id));
    match result {
        Ok(Ok(body)) => {
            rec.extend(body);
            rec.insert("error".into(), Value::Null);
        }
        Ok(Err(e)) => error_record(&mut rec, job, e),
        Err(panic) => {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            error_record(&mut rec, job, format!("internal error: {msg}"));
        }
    }
    rec
}

fn error_record(rec: &mut Map<String, Value>, job: &Job, e: String) {
    rec.insert("schema_version".into(), json!(SCHEMA_VERSION));
    rec.insert(
        "tool".into(),
        json!({"name": TOOL_NAME, "version": TOOL_VERSION}),
    );
    rec.insert("skin".into(), json!({"input": job.path.to_string_lossy()}));
    rec.insert("error".into(), json!(e));
}

pub struct Summary {
    pub total: usize,
    pub skipped: usize,
    pub ok: usize,
    pub errors: usize,
}

pub fn run(jobs: Vec<Job>, opts: &ScanOptions) -> Result<Summary, String> {
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let done = if opts.resume {
        done_ids(&opts.output)
    } else {
        HashSet::new()
    };
    let total = jobs.len();
    let todo: Vec<Job> = jobs.into_iter().filter(|j| !done.contains(&j.id)).collect();
    let skipped = total - todo.len();

    let mut out = if opts.resume {
        BufWriter::new(
            open_append(&opts.output)
                .map_err(|e| io(&opts.output, e))?
                .0,
        )
    } else {
        BufWriter::new(File::create(&opts.output).map_err(|e| io(&opts.output, e))?)
    };
    let mut csv = match &opts.csv {
        Some(p) => {
            let (f, empty) = if opts.resume {
                open_append(p).map_err(|e| io(p, e))?
            } else {
                (File::create(p).map_err(|e| io(p, e))?, true)
            };
            let mut w = BufWriter::new(f);
            if empty {
                w.write_all(b"id,key_mode,note_shapes,ln_cut,unclassified_columns\n")
                    .map_err(|e| io(p, e))?;
            }
            Some(w)
        }
        None => None,
    };

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.threads)
        .build()
        .map_err(|e| e.to_string())?;
    let (tx, rx) = crossbeam_channel::bounded::<Map<String, Value>>(256);
    let n = todo.len();
    let writer = std::thread::scope(|scope| {
        let handle = scope.spawn(move || -> Result<(usize, usize), String> {
            let (mut ok, mut errors) = (0, 0);
            let start = Instant::now();
            let mut last = Instant::now();
            let tty = std::io::stderr().is_terminal();
            for (i, rec) in rx.iter().enumerate() {
                let id = rec["id"].as_str().unwrap().to_owned();
                if rec["error"].is_null() {
                    ok += 1
                } else {
                    errors += 1
                }
                serde_json::to_writer(&mut out, &rec).map_err(|e| e.to_string())?;
                out.write_all(b"\n")
                    .and_then(|_| out.flush())
                    .map_err(|e| io(&opts.output, e))?;
                if let (Some(w), Some(p)) = (csv.as_mut(), opts.csv.as_deref()) {
                    w.write_all(csv_rows(&id, &rec).as_bytes())
                        .and_then(|_| w.flush())
                        .map_err(|e| io(p, e))?;
                }
                let done = i + 1;
                if done == n || last.elapsed() >= Duration::from_secs(if tty { 1 } else { 10 }) {
                    last = Instant::now();
                    let rate = done as f64 / start.elapsed().as_secs_f64().max(1e-9);
                    let eta = (n - done) as f64 / rate.max(1e-9);
                    let line =
                        format!("{done}/{n} skins, {rate:.1}/s, {errors} errors, ETA {eta:.0}s");
                    if tty {
                        eprint!("\r{line}   ")
                    } else {
                        eprintln!("{line}")
                    }
                }
            }
            if tty && n > 0 {
                eprintln!();
            }
            Ok((ok, errors))
        });
        pool.install(|| {
            todo.par_iter().for_each_with(tx, |tx, job| {
                let _ = tx.send(classify_job(job, opts));
            })
        });
        handle.join().unwrap()
    })?;
    Ok(Summary {
        total,
        skipped,
        ok: writer.0,
        errors: writer.1,
    })
}
