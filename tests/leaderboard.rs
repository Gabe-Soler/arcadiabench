//! The leaderboard's entries (`leaderboard/entries/<id>/<version>/`) and
//! rounds (`leaderboard/rounds/`): every entry file matches its schema, its
//! results are a valid report that agrees with its per-seed scores, every
//! round's file matches its schema and its entries have their results on its
//! seeds, and the board built from them matches the board's schema.
//!
//! Entries and rounds are added by pull requests the runner writes, so these
//! tests check what always holds and pin no list, order or digest.

mod support;

use arcadiabench::presets;
use arcadiabench::sim::params::{Catalog, CrewMix};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn validator(schema: &str) -> jsonschema::Validator {
    jsonschema::validator_for(&json(&root().join("schemas").join(schema))).unwrap()
}

fn dirs(path: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    found.sort();
    found
}

/// Every entry's directory, with its id and version from the path.
fn entries() -> Vec<(String, String, PathBuf)> {
    let mut found = Vec::new();
    for id in dirs(&root().join("leaderboard/entries")) {
        for version in dirs(&id) {
            let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
            found.push((name(&id), name(&version), version));
        }
    }
    found
}

/// The four entries the board opened with. They are always there; others
/// come and go, so nothing below counts on the list being these alone.
const FOUNDING: [&str; 4] = [
    "arcadiaos",
    "baseline-fcfs",
    "baseline-greedy",
    "baseline-random",
];

/// An entry's results in `dir` (its `public/`, or a round's directory): a
/// valid report under the entry's own name that covers its tracks, and
/// per-seed scores on `seeds` that are the runs behind the nominal headlines.
fn check_results(what: &str, entry: &Value, dir: &Path, seeds: &[Value]) {
    let tracks: Vec<&str> = entry["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    let catalog = json(&root().join("leaderboard/config.json"))["catalog"].clone();

    let report = json(&dir.join("report.json"));
    if let Err(e) = support::scorecard_validator("Report").validate(&report) {
        panic!("{what} report.json: {e} at {}", e.instance_path());
    }
    let cards = report["scorecards"].as_array().unwrap();
    let fleet_cards = cards.iter().filter(|c| !c["crew"].is_null()).count();
    assert_eq!(
        cards.len() - fleet_cards,
        if tracks.contains(&"standard") { 18 } else { 0 },
        "{what}: the Standard track"
    );
    assert_eq!(
        fleet_cards,
        if tracks.contains(&"fleet") { 6 } else { 0 },
        "{what}: the fleet design track"
    );
    for card in cards {
        // Its own runs, on the catalog the board is on.
        assert_eq!(card["coordinator"]["name"], entry["id"], "{what}");
        assert_eq!(card["coordinator"]["version"], entry["version"], "{what}");
        assert_eq!(card["catalog"], catalog, "{what}");
        if !card["crew"].is_null() {
            assert_eq!(card["crew"], entry["fleet"]["crew"], "{what}");
        }
    }

    let runs = json(&dir.join("runs.json"));
    let scenarios = runs["scenarios"].as_object().unwrap();
    assert_eq!(
        scenarios.len(),
        3 * usize::from(tracks.contains(&"standard")) + usize::from(tracks.contains(&"fleet")),
        "{what}"
    );
    for (scenario, run) in scenarios {
        let scores: Vec<f64> = run["scores"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_f64().unwrap())
            .collect();
        assert_eq!(run["seeds"].as_array().unwrap(), seeds, "{what} {scenario}");
        assert_eq!(scores.len(), seeds.len(), "{what} {scenario}");
        assert!(
            scores.iter().all(|s| (0.0..=1.0).contains(s)),
            "{what} {scenario}"
        );
        let card = cards
            .iter()
            .find(|c| c["scenario_id"] == scenario.as_str())
            .unwrap_or_else(|| panic!("{what}: no scorecard for {scenario}"));
        assert_eq!(card["variant"], "nominal", "{what} {scenario}");
        let mean = scores.iter().sum::<f64>() / scores.len() as f64;
        let headline = card["headline"].as_f64().unwrap();
        assert!(
            (mean - headline).abs() < 1e-12,
            "{what} {scenario}: {mean} against {headline}"
        );
    }
}

#[test]
fn every_entry_matches_its_schema_and_its_results_agree() {
    let entry_schema = validator("leaderboard-entry.schema.json");
    let found = entries();
    for id in FOUNDING {
        assert!(
            found
                .iter()
                .any(|e| e.0 == id && e.1 == "1.1.0" && e.2.join("public/report.json").is_file()),
            "{id} 1.1.0 and its public results"
        );
    }
    let public_seeds: Vec<Value> = (1..=50).map(Value::from).collect();
    for (id, version, dir) in &found {
        let entry = json(&dir.join("entry.json"));
        if let Err(e) = entry_schema.validate(&entry) {
            panic!("{id} {version} entry.json: {e} at {}", e.instance_path());
        }
        // An entry is where its file says it is.
        assert_eq!(
            (entry["id"].as_str(), entry["version"].as_str()),
            (Some(id.as_str()), Some(version.as_str()))
        );

        // An entry waits without results until the runner accepts it. Once
        // it has an image, it has its results on the public seeds.
        let has_results = dir.join("public/report.json").is_file();
        if entry.get("image_digest").is_some() {
            assert!(has_results, "{id} {version} has an image and no results");
        }
        if has_results {
            check_results(
                &format!("{id} {version} public"),
                &entry,
                &dir.join("public"),
                &public_seeds,
            );
        }

        // A crew is one the track admits, at the mass the entry states.
        let on_fleet = entry["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "fleet");
        if on_fleet {
            let crew: CrewMix = serde_json::from_value(entry["fleet"]["crew"].clone()).unwrap();
            presets::check_crew(Catalog::V1_1, crew).unwrap();
            assert_eq!(
                entry["fleet"]["mass_kg"].as_f64().unwrap(),
                presets::crew_mass_kg(Catalog::V1_1, crew),
                "{id}"
            );
        } else {
            assert!(entry.get("fleet").is_none(), "{id}");
        }
        // A replay an entry names is a file in the repository.
        if let Some(replays) = entry.get("replays").and_then(Value::as_object) {
            for path in replays.values() {
                assert!(
                    root().join(path.as_str().unwrap()).is_file(),
                    "{id}: {path}"
                );
            }
        }
    }
}

#[test]
fn every_round_matches_its_schema_and_its_entries_have_their_results() {
    // Nothing to check until a round has been published.
    let base = root().join("leaderboard/rounds");
    if !base.is_dir() {
        return;
    }
    let schema = validator("leaderboard-round.schema.json");
    let mut files: Vec<PathBuf> = fs::read_dir(&base)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    for file in files {
        let round = json(&file);
        if let Err(e) = schema.validate(&round) {
            panic!("{}: {e} at {}", file.display(), e.instance_path());
        }
        let tag = round["tag"].as_str().unwrap();
        assert_eq!(tag, format!("round-{}", round["round"]), "{tag}");
        assert_eq!(file.file_stem().unwrap().to_str(), Some(tag));
        let seeds = round["seeds"].as_array().unwrap();
        let mut ranked = Vec::new();
        for e in round["entries"].as_array().unwrap() {
            let (id, version) = (e["id"].as_str().unwrap(), e["version"].as_str().unwrap());
            let dir = root().join("leaderboard/entries").join(id).join(version);
            let entry = json(&dir.join("entry.json"));
            // The image the round ran is the one the entry was accepted with.
            assert_eq!(entry["image_digest"], e["image_digest"], "{tag}: {id}");
            check_results(
                &format!("{id} {version} {tag}"),
                &entry,
                &dir.join(tag),
                seeds,
            );
            ranked.push((id, version));
        }
        // No entry is both ranked and not.
        for e in round["not_ranked"].as_array().unwrap() {
            let key = (e["id"].as_str().unwrap(), e["version"].as_str().unwrap());
            assert!(!ranked.contains(&key), "{tag}: {key:?}");
        }
    }
}

#[test]
fn the_entry_schema_refuses_what_the_tools_must_not_be_given() {
    let schema = validator("leaderboard-entry.schema.json");
    let good = json(&root().join("leaderboard/entries/baseline-random/1.1.0/entry.json"));
    assert!(schema.is_valid(&good));
    let with = |path: &[&str], value: Value| {
        let mut entry = good.clone();
        let mut at = &mut entry;
        for key in &path[..path.len() - 1] {
            at = &mut at[*key];
        }
        at[path[path.len() - 1]] = value;
        entry
    };
    for (what, bad) in [
        ("an id with a path in it", with(&["id"], "../x".into())),
        ("an id with a space", with(&["id"], "my entry".into())),
        (
            "a branch for a commit",
            with(&["source", "commit"], "main".into()),
        ),
        (
            "a short hash",
            with(&["source", "commit"], "7db45ce".into()),
        ),
        (
            "a repository elsewhere",
            with(&["source", "repository"], "https://example.com/a/b".into()),
        ),
        (
            "a repository with a command in it",
            with(
                &["source", "repository"],
                "https://github.com/a/b; rm -rf /".into(),
            ),
        ),
        (
            "plain http",
            with(&["source", "repository"], "http://github.com/a/b".into()),
        ),
        (
            "an unknown track",
            with(&["tracks"], serde_json::json!(["standard", "speedrun"])),
        ),
        ("no track", with(&["tracks"], serde_json::json!([]))),
        (
            "a field nobody defined",
            with(&["script"], "<script>".into()),
        ),
        (
            "a name of 200 characters",
            with(&["name"], "x".repeat(200).into()),
        ),
        (
            "a replay outside results",
            with(
                &["replays"],
                serde_json::json!({"standard": "https://example.com/x.jsonl.gz"}),
            ),
        ),
    ] {
        assert!(!schema.is_valid(&bad), "{what} was accepted");
    }
}

#[test]
fn the_board_built_from_the_entries_matches_its_schema() {
    // The board is built by a Node script, as the Pages workflow does it.
    let dir = support::scratch_dir("leaderboard");
    let out = dir.join("board.json");
    let built = Command::new("node")
        .arg(root().join("site/tools/build-board.mjs"))
        .arg(&out)
        .output();
    let built = match built {
        Ok(o) => o,
        Err(e) => {
            eprintln!(
                "the_board_built_from_the_entries_matches_its_schema: skipped, node is not available ({e})"
            );
            return;
        }
    };
    assert!(
        built.status.success(),
        "build-board failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let board = json(&out);
    if let Err(e) = validator("leaderboard.schema.json").validate(&board) {
        panic!("board.json: {e} at {}", e.instance_path());
    }
    let rows = |track: &str| board["tracks"][track]["rows"].as_array().unwrap().clone();
    let ids = |track: &str| -> Vec<String> {
        rows(track)
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect()
    };
    // Entries and rounds are added to the board, so only what always holds
    // is checked: the founding entries are on it, every ranked row has a
    // rank, and ranks start at one and never go down.
    let waiting = |track: &str| -> Vec<String> {
        board["tracks"][track]["pending"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|r| r["id"].as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    for id in FOUNDING {
        assert!(
            ids("standard")
                .iter()
                .chain(&waiting("standard"))
                .any(|x| x == id),
            "{id} is not on the Standard track"
        );
    }
    for id in ["arcadiaos", "baseline-greedy"] {
        assert!(
            ids("fleet")
                .iter()
                .chain(&waiting("fleet"))
                .any(|x| x == id),
            "{id} is not on the fleet design track"
        );
    }
    for track in ["standard", "fleet"] {
        let ranks: Vec<u64> = rows(track)
            .iter()
            .map(|r| r["rank"].as_u64().expect("a ranked row has a rank"))
            .collect();
        assert_eq!(ranks.first(), Some(&1), "{track}");
        assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "{track}: {ranks:?}");
    }
    // The same entries give the same file.
    let again = dir.join("board-again.json");
    let status = Command::new("node")
        .arg(root().join("site/tools/build-board.mjs"))
        .arg(&again)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fs::read(&out).unwrap(), fs::read(&again).unwrap());
}
