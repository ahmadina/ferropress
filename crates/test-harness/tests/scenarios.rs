//! Every scenario under `contracts/scenarios/` must parse, validate and be named after its file.

use std::fs;
use std::path::PathBuf;

use test_harness::Scenario;

fn scenario_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/scenarios")
}

#[test]
fn committed_scenarios_are_valid_and_named_after_their_files() {
    let mut checked = 0;
    for entry in fs::read_dir(scenario_dir()).unwrap() {
        let path = entry.unwrap().path();
        assert_eq!(
            path.extension().and_then(|e| e.to_str()),
            Some("toml"),
            "{} is not a scenario file",
            path.display()
        );
        let text = fs::read_to_string(&path).unwrap();
        let scenario = Scenario::from_toml(&text)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap();
        assert_eq!(scenario.id().as_str(), stem, "{}", path.display());
        checked += 1;
    }
    assert!(checked > 0, "no scenarios found");
}
