//! End-to-end checks on synthetic skins (mirrors tests/test_classify.py).

use image::{ImageBuffer, Rgba as Px};
use mania_skin_classify::classify::classify_skin;
use mania_skin_classify::config::Thresholds;
use mania_skin_classify::imageio::Rgba;
use mania_skin_classify::raster::{Gray, fill_polygon};
use mania_skin_classify::shapes::{classify_mask, opaque_mask};
use mania_skin_classify::sources::SkinSource;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

fn tmpdir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "msc-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn sprite(mask: &Gray) -> Rgba {
    let mut data = Vec::with_capacity(mask.data.len() * 4);
    for &v in &mask.data {
        data.extend_from_slice(&[200, 120, 255, v]);
    }
    Rgba {
        width: mask.w,
        height: mask.h,
        data,
    }
}

fn disc(size: usize, ring: Option<f64>) -> Gray {
    let mut g = Gray::new(size, size);
    let c = size as f64 / 2.0;
    let r = c - 6.0;
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f64 + 0.5 - c).powi(2) + (y as f64 + 0.5 - c).powi(2)).sqrt();
            if d <= r && ring.is_none_or(|w| d >= r - w) {
                g.data[y * size + x] = 255;
            }
        }
    }
    g
}

fn poly(w: usize, h: usize, pts: &[(f64, f64)]) -> Gray {
    fill_polygon(w, h, pts, 1)
}

fn save(path: &Path, img: &Rgba) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    ImageBuffer::<Px<u8>, _>::from_raw(img.width as u32, img.height as u32, img.data.clone())
        .unwrap()
        .save(path)
        .unwrap();
}

fn percy_body(width: usize, height: usize, blank: usize) -> Rgba {
    let mut data = vec![0u8; width * height * 4];
    for y in blank..height {
        for x in 8..width - 8 {
            data[(y * width + x) * 4..(y * width + x) * 4 + 4]
                .copy_from_slice(&[180, 180, 180, 255]);
        }
    }
    Rgba {
        width,
        height,
        data,
    }
}

fn shape_of(mask: &Gray) -> &'static str {
    let t = Thresholds::default();
    let (m, _) = opaque_mask(&sprite(mask), &t);
    classify_mask(&m, mask.w, mask.h, &t).shape
}

#[test]
fn shape_classes() {
    assert_eq!(shape_of(&disc(128, None)), "circle");
    assert_eq!(shape_of(&disc(128, Some(12.0))), "circle");
    assert_eq!(
        shape_of(&poly(
            128,
            128,
            &[
                (8.0, 64.0),
                (64.0, 8.0),
                (64.0, 40.0),
                (120.0, 40.0),
                (120.0, 88.0),
                (64.0, 88.0),
                (64.0, 120.0)
            ]
        )),
        "arrow"
    );
    assert_eq!(
        shape_of(&poly(
            128,
            128,
            &[
                (64.0, 10.0),
                (120.0, 66.0),
                (120.0, 110.0),
                (64.0, 54.0),
                (8.0, 110.0),
                (8.0, 66.0)
            ]
        )),
        "arrow"
    );
    assert_eq!(
        shape_of(&poly(
            160,
            48,
            &[(4.0, 4.0), (156.0, 4.0), (156.0, 43.0), (4.0, 43.0)]
        )),
        "bar"
    );
    assert_eq!(
        shape_of(&poly(
            128,
            128,
            &[(64.0, 6.0), (122.0, 64.0), (64.0, 122.0), (6.0, 64.0)]
        )),
        "etc"
    );
    assert_eq!(
        shape_of(&poly(
            128,
            128,
            &[(10.0, 10.0), (118.0, 10.0), (118.0, 118.0), (10.0, 118.0)]
        )),
        "etc"
    );
    assert_eq!(shape_of(&Gray::new(64, 64)), "unknown");
}

fn ln_skin(ini_extra: &str, bodies: &[(&str, Rgba)], tail_transparent: bool) -> PathBuf {
    let root = tmpdir();
    std::fs::write(
        root.join("skin.ini"),
        format!("[Mania]\nKeys: 4\n{ini_extra}\n"),
    )
    .unwrap();
    save(&root.join("mania-note1.png"), &sprite(&disc(128, None)));
    save(&root.join("mania-note2.png"), &sprite(&disc(128, None)));
    let tail = if tail_transparent {
        Rgba {
            width: 64,
            height: 32,
            data: vec![0; 64 * 32 * 4],
        }
    } else {
        sprite(&disc(64, None))
    };
    save(&root.join("mania-note1T.png"), &tail);
    save(&root.join("mania-note2T.png"), &tail);
    for (name, img) in bodies {
        save(&root.join(name), img);
    }
    root
}

fn run(root: &Path) -> serde_json::Map<String, Value> {
    classify_skin(
        &SkinSource::open(root).unwrap(),
        &Thresholds::default(),
        true,
        false,
    )
    .unwrap()
}

fn ln_cut(root: &Path) -> String {
    run(root)["key_modes"]["4"]["ln_cut"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn percy_cut_rules() {
    let both = |b: Rgba| [("mania-note1L.png", b.clone()), ("mania-note2L.png", b)];
    let r = run(&ln_skin("", &both(percy_body(64, 4000, 50)), true));
    assert_eq!(
        r["key_modes"]["4"],
        serde_json::json!({"note_shapes": ["circle"], "ln_cut": "all", "unclassified_columns": []})
    );
    assert_eq!(
        ln_cut(&ln_skin(
            "NoteBodyStyle: 0",
            &both(percy_body(64, 4000, 50)),
            true
        )),
        "none"
    );
    assert_eq!(
        ln_cut(&ln_skin(
            "NoteBodyStyle: 1\nNoteBodyStyle0: 0",
            &both(percy_body(64, 4000, 50)),
            true
        )),
        "mixed"
    );
    assert_eq!(
        ln_cut(&ln_skin("", &both(percy_body(64, 4000, 0)), true)),
        "none"
    );
    // 1800 px @2x is 900 logical px: below the 1000 px minimum.
    let small = [
        ("mania-note1L@2x.png", percy_body(64, 1800, 100)),
        ("mania-note2L@2x.png", percy_body(64, 1800, 100)),
    ];
    assert_eq!(ln_cut(&ln_skin("", &small, true)), "none");
}

#[test]
fn ghost_end_and_cover_band_are_background() {
    let mut ghost = percy_body(64, 4000, 0);
    for y in 0..60 {
        for x in 8..56 {
            ghost.data[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4]
                .copy_from_slice(&[200, 200, 200, 30]);
        }
    }
    let mut band = percy_body(64, 4000, 0);
    for p in band.data[..70 * 64 * 4].as_chunks_mut::<4>().0 {
        *p = [0, 0, 0, 255];
    }
    let r = run(&ln_skin(
        "",
        &[("mania-note1L.png", ghost), ("mania-note2L.png", band)],
        true,
    ));
    let blank = |c: usize| {
        r["details"]["4"][c]["ln"]["body_top_blank_px"]
            .as_f64()
            .unwrap()
    };
    assert_eq!((blank(0), blank(1)), (60.0, 70.0));
    assert_eq!(r["key_modes"]["4"]["ln_cut"], "all");
}

#[test]
fn missing_bodies_and_notes_are_unknown_not_guessed() {
    let r = run(&ln_skin(
        "",
        &[("mania-note1L.png", percy_body(64, 4000, 50))],
        true,
    ));
    assert_eq!(r["key_modes"]["4"]["ln_cut"], "unknown");
    let missing: Vec<u64> = r["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|u| u["role"] == "ln_body")
        .map(|u| u["column"].as_u64().unwrap())
        .collect();
    assert_eq!(missing, vec![1, 2]);

    let root = tmpdir();
    std::fs::write(
        root.join("skin.ini"),
        "[Mania]\nKeys: 2\nNoteImage1: nope\n",
    )
    .unwrap();
    save(&root.join("mania-note1.png"), &sprite(&disc(128, None)));
    let r = run(&root);
    assert_eq!(
        r["key_modes"]["2"]["note_shapes"],
        serde_json::json!(["circle"])
    );
    assert_eq!(
        r["key_modes"]["2"]["unclassified_columns"],
        serde_json::json!([1])
    );
}

#[test]
fn osk_and_folder_agree() {
    let root = ln_skin(
        "",
        &[
            ("mania-note1L.png", percy_body(64, 4000, 50)),
            ("mania-note2L.png", percy_body(64, 4000, 50)),
        ],
        true,
    );
    let osk = root.with_extension("osk");
    let mut zw = zip::ZipWriter::new(std::fs::File::create(&osk).unwrap());
    for e in std::fs::read_dir(&root).unwrap() {
        let p = e.unwrap().path();
        // Wrap in a top-level folder: the archive must be re-rooted at the single skin.ini.
        zw.start_file(
            format!("Wrapped Skin/{}", p.file_name().unwrap().to_string_lossy()),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(&mut zw, &std::fs::read(&p).unwrap()).unwrap();
    }
    zw.finish().unwrap();
    let t = Thresholds::default();
    let a = classify_skin(&SkinSource::open(&root).unwrap(), &t, false, true).unwrap();
    let b = classify_skin(&SkinSource::open(&osk).unwrap(), &t, false, true).unwrap();
    assert_eq!(a["key_modes"], b["key_modes"]);
    assert_eq!(a["skin"]["content_hash"], b["skin"]["content_hash"]);
}
