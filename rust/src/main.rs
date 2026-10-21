use clap::{Args, Parser, Subcommand};
use mania_skin_classify::classify::classify_skin;
use mania_skin_classify::config::Thresholds;
use mania_skin_classify::scan::{self, ScanOptions};
use mania_skin_classify::sources::SkinSource;
use std::path::PathBuf;
use std::process::ExitCode;

/// Classify osu!mania skins: note shapes and cut (Percy) LNs per declared key mode.
///
/// `mania-skin-classify SKIN` classifies one skin folder or .osk and prints JSON.
/// `mania-skin-classify scan ...` classifies many skins in parallel into JSONL.
#[derive(Parser)]
#[command(version, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    single: SingleArgs,
}

#[derive(Args)]
struct SingleArgs {
    /// One skin folder (containing skin.ini) or one .osk archive
    skin: Option<PathBuf>,
    /// Write JSON here instead of stdout
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Include per-column evidence and the thresholds used
    #[arg(long)]
    details: bool,
    /// JSON file overriding default thresholds
    #[arg(long)]
    thresholds: Option<PathBuf>,
    /// Skip hashing every skin file for skin.content_hash
    #[arg(long)]
    no_content_hash: bool,
    /// Single-line JSON
    #[arg(long)]
    compact: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Classify every skin under a directory (or from a list) into JSONL, in parallel
    Scan(ScanArgs),
}

#[derive(Args)]
struct ScanArgs {
    /// Directory searched recursively for *.osk files and folders containing skin.ini
    #[arg(required_unless_present = "list", conflicts_with = "list")]
    dir: Option<PathBuf>,
    /// TSV file of `id<TAB>path` lines (e.g. exported from the site database)
    #[arg(long)]
    list: Option<PathBuf>,
    /// JSONL output, one line per skin
    #[arg(short, long)]
    output: PathBuf,
    /// Also write flat rows id,key_mode,note_shapes,ln_cut,unclassified_columns
    #[arg(long)]
    csv: Option<PathBuf>,
    /// Worker threads (default: all cores)
    #[arg(short = 'j', long)]
    jobs: Option<usize>,
    /// Append to existing output, skipping ids already classified without error
    #[arg(long)]
    resume: bool,
    /// Include per-column evidence in every record
    #[arg(long)]
    details: bool,
    /// Compute skin.content_hash (reads every file of every skin)
    #[arg(long)]
    content_hash: bool,
    /// JSON file overriding default thresholds
    #[arg(long)]
    thresholds: Option<PathBuf>,
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("mania-skin-classify: {msg}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Scan(a)) => {
            let thresholds = match Thresholds::load(a.thresholds.as_deref()) {
                Ok(t) => t,
                Err(e) => return fail(e),
            };
            let jobs = match (&a.dir, &a.list) {
                (Some(d), _) => scan::discover(d).map_err(|e| format!("{}: {e}", d.display())),
                (None, Some(l)) => scan::read_list(l),
                (None, None) => unreachable!("clap requires dir or --list"),
            };
            let jobs = match jobs {
                Ok(j) => j,
                Err(e) => return fail(e),
            };
            let opts = ScanOptions {
                output: a.output,
                csv: a.csv,
                threads: a.jobs.unwrap_or(0),
                resume: a.resume,
                details: a.details,
                content_hash: a.content_hash,
                thresholds,
            };
            match scan::run(jobs, &opts) {
                Ok(s) => {
                    eprintln!(
                        "{} skins: {} classified, {} errors, {} skipped (already done)",
                        s.total, s.ok, s.errors, s.skipped
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => fail(e),
            }
        }
        None => {
            let a = cli.single;
            let Some(skin) = a.skin else {
                return fail("missing SKIN (or use `scan`); see --help");
            };
            let thresholds = match Thresholds::load(a.thresholds.as_deref()) {
                Ok(t) => t,
                Err(e) => return fail(e),
            };
            let src = match SkinSource::open(&skin) {
                Ok(s) => s,
                Err(e) => return fail(e),
            };
            let result = match classify_skin(&src, &thresholds, a.details, !a.no_content_hash) {
                Ok(r) => serde_json::Value::Object(r),
                Err(e) => return fail(e),
            };
            let text = if a.compact {
                serde_json::to_string(&result)
            } else {
                serde_json::to_string_pretty(&result)
            }
            .unwrap();
            match a.output {
                Some(p) => {
                    if let Err(e) = std::fs::write(&p, text + "\n") {
                        return fail(format!("{}: {e}", p.display()));
                    }
                }
                None => println!("{text}"),
            }
            ExitCode::SUCCESS
        }
    }
}
