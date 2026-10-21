//! Tunable thresholds. Field names and defaults match `config.py`; the JSON override file
//! format is shared with the Python reference.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Thresholds {
    // note silhouette extraction
    pub alpha_floor: i64,
    pub alpha_rel: f64,
    pub opaque_bg_tolerance: i64,
    pub close_frac: f64,
    pub component_min_frac: f64,
    // note shape decision
    pub fragment_min_frac: f64,
    pub circle_max_aspect: f64,
    pub bar_min_aspect: f64,
    pub match_min_iou: f64,
    pub ambiguity_margin: f64,
    pub arrow_max_point_symmetry: f64,
    // cut (Percy) LN detection
    pub percy_body_style: i64,
    pub percy_min_height: f64,
    pub percy_min_top_blank_px: f64,
    pub blank_alpha: i64,
    pub ghost_alpha_rel: f64,
    pub body_min_run_px: f64,
    pub band_colour_tol: i64,
    pub top_scan_rows: usize,
    pub tail_transparent_alpha: i64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            alpha_floor: 24,
            alpha_rel: 0.45,
            opaque_bg_tolerance: 40,
            close_frac: 0.02,
            component_min_frac: 0.05,
            fragment_min_frac: 0.6,
            circle_max_aspect: 1.35,
            bar_min_aspect: 1.3,
            match_min_iou: 0.8,
            ambiguity_margin: 0.02,
            arrow_max_point_symmetry: 0.93,
            percy_body_style: 1,
            percy_min_height: 1000.0,
            percy_min_top_blank_px: 4.0,
            blank_alpha: 8,
            ghost_alpha_rel: 0.25,
            body_min_run_px: 32.0,
            band_colour_tol: 8,
            top_scan_rows: 4096,
            tail_transparent_alpha: 8,
        }
    }
}

impl Thresholds {
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}
