//! Two captures of the same request on the reference lab, taken from the same state, must match after
//! the scenario's reviewed normalizations; anything else is nondeterminism the harness does not yet
//! account for.
//!
//! The captures follow a warm-up request because the first request after an install schedules events
//! and creates content stamped with the wall-clock time; comparing first visits needs a frozen clock
//! on both sides. Needs the running lab and fresh captures (git-ignored, so not available in CI):
//!
//! ```sh
//! cargo xtask lab reset
//! cargo xtask lab capture warm-up /
//! cargo xtask lab capture front-page-a /
//! cargo xtask lab capture front-page-b /
//! cargo test -p test-harness --test lab_repeatability -- --ignored
//! ```

use std::fs;
use std::path::PathBuf;

use test_harness::{Observation, Scenario, Slug, Step, Target, run};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Serves one capture file per step, in order.
struct CaptureFiles(Vec<PathBuf>);

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct CaptureError(String);

impl Target for CaptureFiles {
    type Error = CaptureError;

    fn observe(&mut self, _: &Slug, index: usize, _: &Step) -> Result<Observation, CaptureError> {
        let path = self
            .0
            .get(index)
            .ok_or_else(|| CaptureError(format!("no capture for step {index}")))?;
        let text = fs::read_to_string(path)
            .map_err(|error| CaptureError(format!("{}: {error}", path.display())))?;
        serde_json::from_str(&text)
            .map_err(|error| CaptureError(format!("{}: {error}", path.display())))
    }
}

#[test]
#[ignore = "needs fresh lab captures; see the module docs"]
fn consecutive_front_page_captures_match_after_normalization() {
    let root = repo_root();
    let text = fs::read_to_string(root.join("contracts/scenarios/front-page.toml")).unwrap();
    let scenario = Scenario::from_toml(&text).unwrap();
    let captures = root.join("lab/captures");
    let mut first = CaptureFiles(vec![captures.join("front-page-a.json")]);
    let mut second = CaptureFiles(vec![captures.join("front-page-b.json")]);

    let report = run(&scenario, &mut first, &mut second).unwrap();
    let mismatches: Vec<String> = report
        .steps
        .iter()
        .flat_map(|step| step.mismatches.iter().map(ToString::to_string))
        .collect();
    assert!(report.passed(), "{mismatches:#?}");
}
