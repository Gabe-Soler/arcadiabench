//! The baseline coordinators: honest protocol clients that build the pad
//! on every seed they are given, deterministically, without breaking the
//! authority policy.

mod support;

use arcadia_protocol::*;
use arcadiabench::baselines::{Allocation, Baseline};
use arcadiabench::coordinator::Agent;
use arcadiabench::log::EventLog;
use arcadiabench::run::run_process;
use arcadiabench::session::RunReport;
use arcadiabench::sim::params::{Params, Preset, Variant};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A baseline run in process, under the same schema checks as a subprocess.
struct InProc(Baseline);

impl Agent for InProc {
    fn welcome(&mut self, hello: &Hello) -> SoftwareInfo {
        self.0.welcome(hello)
    }
    fn decide(&mut self, obs: &Observation) -> Decision {
        self.0.decide(obs)
    }
}

fn run(how: Allocation, seed: u64, params: &Params) -> (RunReport, String) {
    let (report, text, _) = support::run_raw(InProc(Baseline::new(how)), seed, params);
    (report, text)
}

#[test]
fn every_baseline_builds_small_nominal() {
    for how in Allocation::ALL {
        for seed in 1..=3 {
            let (report, text) = run(how, seed, &Params::default());
            let m = &report.metrics;
            assert_eq!(
                report.end_reason,
                EndReason::GoalsComplete,
                "{how} seed {seed}: ended at sol {:.2}",
                m.sols
            );
            assert!(m.completed && m.quality.verified && m.quality.berm_complete);
            assert!(m.sols < 2.0, "{how} seed {seed}: {:.2} sols", m.sols);
            assert_eq!(m.autonomy.policy_violations, 0, "{how} seed {seed}");
            assert_eq!(m.autonomy.approval_requests, 0, "{how} seed {seed}");
            support::assert_valid_log(&text);
        }
    }
}

#[test]
fn baselines_are_deterministic() {
    for how in Allocation::ALL {
        let params = Params::scenario(Preset::Medium, Variant::Nominal);
        let (a, log_a) = run(how, 5, &params);
        let (b, log_b) = run(how, 5, &params);
        assert_eq!(a, b, "{how}");
        assert_eq!(log_a, log_b, "{how}: byte-identical logs");
    }
}

#[test]
fn the_allocations_differ() {
    // On a crew with several robots of each design the three policies make
    // different choices, so their runs differ.
    let params = Params::scenario(Preset::Medium, Variant::Nominal);
    let logs: Vec<String> = Allocation::ALL
        .iter()
        .map(|&how| run(how, 2, &params).1)
        .collect();
    assert_ne!(logs[0], logs[1]);
    assert_ne!(logs[1], logs[2]);
    assert_ne!(logs[0], logs[2]);
}

#[test]
fn faults_are_handled_and_tows_notified() {
    // Cascading faults on Small: robots break down, and every level-2 tow
    // is reported in the decision that issues it.
    let params = Params::variant(Variant::CascadingFaults);
    for seed in [1, 2] {
        let (report, text) = run(Allocation::Greedy, seed, &params);
        let m = &report.metrics;
        assert_eq!(report.end_reason, EndReason::GoalsComplete, "seed {seed}");
        assert!(m.fleet.faults > 0, "seed {seed}: faults strike");
        assert_eq!(m.autonomy.policy_violations, 0, "seed {seed}");
        let tows = text.matches("\"type\":\"tow\"").count();
        assert!(m.autonomy.notifications as usize >= tows.min(1));
    }
}

#[test]
fn the_binaries_speak_the_protocol() {
    for (how, program) in [
        (Allocation::Random, env!("CARGO_BIN_EXE_baseline-random")),
        (Allocation::Greedy, env!("CARGO_BIN_EXE_baseline-greedy")),
        (Allocation::Fcfs, env!("CARGO_BIN_EXE_baseline-fcfs")),
    ] {
        let mut log = EventLog::new(Vec::new());
        let report = run_process(
            program,
            &[],
            1,
            &Params::default(),
            Duration::from_secs(10),
            &mut log,
        )
        .unwrap();
        assert_eq!(report.end_reason, EndReason::GoalsComplete, "{how}");
        assert_eq!(report.coordinator.name, how.name());
        // The subprocess decides exactly as the in-process baseline does.
        let (_, text) = run(how, 1, &Params::default());
        assert_eq!(String::from_utf8(log.into_inner()).unwrap(), text, "{how}");
    }
}

/// Crates and Bench modules the baselines must never name.
const FORBIDDEN: [&str; 15] = [
    "serde",
    "serde_json",
    "libm",
    "flate2",
    "jsonschema",
    "sim",
    "scenario",
    "presets",
    "bench",
    "score",
    "session",
    "coordinator",
    "log",
    "run",
    "cli",
];

/// Every path a source file names, by its first segment and what follows.
fn paths(code: &str) -> Vec<(String, String)> {
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = Vec::new();
    for line in code.lines() {
        let line = line.split("//").next().unwrap_or("");
        let bytes: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == ':' && bytes[i + 1] == ':' {
                // The identifier just before this `::`.
                let mut j = i;
                while j > 0 && is_ident(bytes[j - 1]) {
                    j -= 1;
                }
                let root_start = j == 0 || !(bytes[j - 1] == ':' || is_ident(bytes[j - 1]));
                if root_start && j < i {
                    let root: String = bytes[j..i].iter().collect();
                    let rest: String = bytes[i + 2..]
                        .iter()
                        .take_while(|&&c| is_ident(c) || c == ':')
                        .collect();
                    out.push((root, rest));
                }
                i += 2;
            } else {
                i += 1;
            }
        }
    }
    out
}

fn baseline_sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("src/baselines"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    for bin in ["random", "greedy", "fcfs"] {
        files.push(root.join(format!("src/bin/baseline-{bin}.rs")));
    }
    files.sort();
    files
}

#[test]
fn baselines_use_only_the_protocol() {
    let files = baseline_sources();
    assert!(files.len() >= 8, "{files:?}");
    for file in files {
        let code = std::fs::read_to_string(&file).unwrap();
        let name = file.display();
        assert!(!code.contains("extern crate"), "{name}");
        assert!(
            !code.contains("super::super"),
            "{name} reaches outside the baselines"
        );
        for (root, rest) in paths(&code) {
            let inside = rest == "baselines" || rest.starts_with("baselines::");
            match root.as_str() {
                "crate" | "arcadiabench" => {
                    assert!(inside, "{name} uses {root}::{rest}");
                }
                r if FORBIDDEN.contains(&r) => panic!("{name} uses {root}::{rest}"),
                _ => {}
            }
        }
        for line in code.lines().map(str::trim) {
            let Some(path) = line
                .strip_prefix("use ")
                .or_else(|| line.strip_prefix("pub use "))
            else {
                continue;
            };
            let root = path.split("::").next().unwrap_or("");
            let allowed = [
                "arcadia_protocol",
                "std",
                "super",
                "self",
                "crate",
                "arcadiabench",
                "alloc",
                "plan",
                "safety",
                "world",
            ];
            // A capitalised root is a type already in scope, such as an enum.
            let in_scope = root.starts_with(|c: char| c.is_ascii_uppercase());
            assert!(allowed.contains(&root) || in_scope, "{name}: use {path}");
        }
    }
}

#[test]
fn the_protocol_check_catches_bench_internals() {
    let bad = "use crate::sim::params::Params;\nlet x = arcadiabench::scenario::generate(1, &p);\nlet y = serde_json::to_string(&v);";
    let found = paths(bad);
    assert!(found.contains(&("crate".into(), "sim::params::Params".into())));
    assert!(found.contains(&("arcadiabench".into(), "scenario::generate".into())));
    assert!(found.iter().any(|(r, _)| r == "serde_json"));
    let good =
        "use arcadia_protocol::*;\nlet a = Action::MoveTo { target };\nlet b = f64::INFINITY;";
    assert!(
        paths(good)
            .iter()
            .all(|(r, _)| !FORBIDDEN.contains(&r.as_str()) && r != "crate")
    );
}
