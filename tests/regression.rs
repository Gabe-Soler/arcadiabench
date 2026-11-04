//! Existing worlds stay the same: on seeds 1-5 the terrain, the scenario
//! (apart from the fields added since) and the weather are byte-identical to
//! what week 2 produced. The fingerprints are FNV-1a hashes of the raw log
//! text of week-2 runs of the null coordinator (`arcadiabench` 0.1.0,
//! protocol 0.1.0), taken without parsing so they do not depend on how JSON
//! numbers are parsed.
//!
//! The Earth link added in week 6 is part of the world too: its schedule
//! comes from the seed alone, so every coordinator sees the same `comms`
//! events.
//!
//! Week 7's scenario catalog leaves the Small preset as week 6 left it: on
//! seeds 1-10, `small` + `nominal` gives the same terrain, scenario (crew,
//! stations, depots, power, base station, policy, prior) and whole runs,
//! byte for byte, apart from the scenario's new name.
//!
//! Week 9 moved the stress variants' onsets so that they overlap every
//! preset's build. Of the variants fingerprinted here only `conjunction`
//! changed (its blackout now starts 0.01-0.03 sols in), so its runs are
//! pinned as week 9 left them; `dust_storm`, new in week 7, has no week-6
//! runs to keep.
//!
//! No fingerprint covers a software or protocol version. They hash the true
//! site and the scenario, both cut out of `run_start`, and the records after
//! `run_start`, which carry no versions ([`week6_fingerprints`] checks this).
//! So the 1.0.0 release, which changed only those versions, left every
//! constant here as it was.

mod support;

use arcadia_protocol::*;
use arcadiabench::coordinator::{Agent, InProcess};
use arcadiabench::log::EventLog;
use arcadiabench::session::{RunReport, run_session};
use arcadiabench::sim::params::{Params, Preset, Variant};

/// (seed, true_site text, scenario text, weather event texts, number of weather events).
const WEEK2: [(u64, u64, u64, u64, usize); 5] = [
    (
        1,
        0xe1580bf28fcba2e5,
        0xc5f993ae54e7c8cc,
        0xbdba886e98765c7a,
        17,
    ),
    (
        2,
        0xfbdc4eb8a339bb5a,
        0xd770939727ce05a4,
        0x322b4d903e7fde6b,
        1,
    ),
    (
        3,
        0x425737f8e58e723c,
        0x8fdfba4cf0431244,
        0xda88f37707b14add,
        9,
    ),
    (
        4,
        0x65a2c189dad7f274,
        0xf81c9e97b296dc89,
        0x8c7981663c23ab3f,
        16,
    ),
    (
        5,
        0x1d8d286c0f59efef,
        0x5b8f88e89b9cb318,
        0xafebf1cbf459957f,
        15,
    ),
];

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The raw text between two markers of a log line.
fn between<'a>(line: &'a str, from: &str, to: &str) -> &'a str {
    let start = line.find(from).expect(from) + from.len();
    let end = start + line[start..].find(to).expect(to);
    &line[start..end]
}

/// Parks every robot on the charge station and keeps it topped up, so the
/// fleet lives through all 60 sols and every storm is reported.
struct Parker {
    capacity: Vec<(String, f64)>,
    next: u64,
}

impl Agent for Parker {
    fn welcome(&mut self, hello: &Hello) -> SoftwareInfo {
        self.capacity = hello
            .scenario
            .robots
            .iter()
            .map(|r| (r.robot_id.clone(), r.battery_capacity_wh))
            .collect();
        SoftwareInfo {
            name: "parker".into(),
            version: "0".into(),
        }
    }

    fn decide(&mut self, obs: &Observation) -> Decision {
        let mut d = Decision::empty(obs.seq);
        for r in &obs.robots {
            let cap = self
                .capacity
                .iter()
                .find(|(id, _)| *id == r.robot_id)
                .map_or(0.0, |(_, c)| *c);
            if r.current_command.is_none()
                && matches!(r.status, RobotStatus::Idle | RobotStatus::Offline)
                && r.battery_wh < 0.9 * cap
            {
                self.next += 1;
                d.commands.push(Command {
                    command_id: format!("p-{}", self.next),
                    robot_id: r.robot_id.clone(),
                    approval_id: None,
                    action: Action::Charge {
                        source: "st-1".into(),
                        until_wh: None,
                    },
                });
            }
        }
        d
    }
}

#[test]
fn seeds_1_to_5_keep_their_week_2_worlds() {
    for (seed, site_fp, scenario_fp, weather_fp, weather_n) in WEEK2 {
        let mut link = InProcess::new(Parker {
            capacity: Vec::new(),
            next: 0,
        })
        .unwrap();
        let mut log = EventLog::new(Vec::new());
        let report = run_session(&mut link, seed, &Params::default(), &mut log).unwrap();
        assert_eq!(report.end_reason, EndReason::TimeLimit, "seed {seed}");
        let text = String::from_utf8(log.into_inner()).unwrap();
        let lines: Vec<&str> = text.lines().collect();

        let start = lines[0];
        let site =
            &start[start.find("\"true_site\":").unwrap() + "\"true_site\":".len()..start.len() - 1];
        assert_eq!(fnv1a(site), site_fp, "seed {seed}: terrain");

        // The scenario minus what v0.2.0 added: power, base_station and the
        // `service` level of the default policy; and under its week-2 name.
        let scenario = week6_scenario(between(start, "\"scenario\":", ",\"true_site\":"));
        let (head, rest) = scenario.split_at(scenario.find(",\"power\":{").unwrap());
        let tail = &rest[rest.find(",\"policy\":{").unwrap()..];
        let week2_shape = format!("{head}{tail}").replacen("\"service\":1,", "", 1);
        assert_eq!(fnv1a(&week2_shape), scenario_fp, "seed {seed}: scenario");

        let weather: Vec<&str> = lines
            .iter()
            .filter(|l| {
                l.contains("\"kind\":\"observed\",\"event\":{")
                    && l.contains(",\"type\":\"weather\",")
            })
            .map(|l| &l[l.find("\"event\":").unwrap() + "\"event\":".len()..l.len() - 1])
            .collect();
        assert_eq!(weather.len(), weather_n, "seed {seed}: weather events");
        assert_eq!(
            fnv1a(&weather.join("\n")),
            weather_fp,
            "seed {seed}: weather"
        );
    }
}

/// A Small scenario's text under the name week 6 gave every variant,
/// `landing-pad-small`.
fn week6_scenario(scenario: &str) -> String {
    let named = between(scenario, "\"scenario_id\":\"", "\"");
    assert!(named.starts_with("landing-pad-small-"), "{named}");
    scenario.replacen(named, "landing-pad-small", 1)
}

/// What week 6 produced on Small (`arcadiabench` 0.1.0 at commit 7c41a92,
/// protocol 0.2.0), as FNV-1a hashes of raw log text: (variant, seed,
/// true_site, scenario, then the rest of the log after `run_start` for a
/// parker run, an idle run and a scripted-builder run). The whole-run
/// hashes pin every outcome; a later change that deliberately alters the
/// outcomes of Small runs must update them and say so. The `conjunction`
/// rows moved to [`WEEK9`] when week 9 moved its blackout.
#[allow(clippy::type_complexity)]
const WEEK6: [(&str, u64, u64, u64, u64, u64, u64); 19] = [
    (
        "nominal",
        1,
        0xe1580bf28fcba2e5,
        0xacfd5b2623089397,
        0x61f535a29c08fe16,
        0xbdc34025e220cb9e,
        0x8526ae763a990fce,
    ),
    (
        "nominal",
        2,
        0xfbdc4eb8a339bb5a,
        0x3ea091df5f37cfff,
        0x4ff106e0d53f5671,
        0x0da5e0da9a82cde4,
        0x3c79d9384b8115d3,
    ),
    (
        "nominal",
        3,
        0x425737f8e58e723c,
        0x196f3a7c7d429cdf,
        0x642f76cb8c62d6a0,
        0xd8d29e212679ff32,
        0x3c4d6fa1521ab43b,
    ),
    (
        "nominal",
        4,
        0x65a2c189dad7f274,
        0xb854e8a20e907eda,
        0x71bb841a680699bf,
        0x9f23f01dafd35b38,
        0x1bc16a8ec68a33f6,
    ),
    (
        "nominal",
        5,
        0x1d8d286c0f59efef,
        0x78184a50b5e7afdb,
        0xe3886c11409a7e57,
        0x61fd034e589e83a7,
        0xf4591de5ae6f7401,
    ),
    (
        "nominal",
        6,
        0x9db2b6ca45164cd7,
        0x4e92a04052ee333f,
        0xe67cebdcd37951e2,
        0x8d827c0d1eff6749,
        0x86d15a58cf8cd041,
    ),
    (
        "nominal",
        7,
        0x8b90451475028882,
        0xa23f38e3e8334412,
        0x3554c939474a80b7,
        0x9545174e444fa36a,
        0xc5e06933980d5c13,
    ),
    (
        "nominal",
        8,
        0x9394c24107f06bc8,
        0xc51910a3fdd94b2f,
        0xc260d849c78558e9,
        0x7312bb62cb6c5e31,
        0xe2bb5e5c7f24c161,
    ),
    (
        "nominal",
        9,
        0x392535327b6a1d56,
        0xe49cb337d070ee6c,
        0x972fe376162c278e,
        0x766700b221e4f12b,
        0x10a326a83f04edb1,
    ),
    (
        "nominal",
        10,
        0xa509b9d75511255a,
        0x0267f73b3bea8dc1,
        0x2d93baae518cad81,
        0x391229ec47e41558,
        0x784dbb60fb675f2a,
    ),
    (
        "cascading_faults",
        1,
        0xe1580bf28fcba2e5,
        0xacfd5b2623089397,
        0xd122a8b56e200de8,
        0xbdc34025e220cb9e,
        0xda06cb1d78f26c49,
    ),
    (
        "cascading_faults",
        2,
        0xfbdc4eb8a339bb5a,
        0x3ea091df5f37cfff,
        0x9ecb190cb0a9e086,
        0x0da5e0da9a82cde4,
        0x735bbbb6f941b5c7,
    ),
    (
        "cascading_faults",
        3,
        0x425737f8e58e723c,
        0x196f3a7c7d429cdf,
        0x462524229a1279dd,
        0xd8d29e212679ff32,
        0x69773a1d141e58df,
    ),
    (
        "solar_only",
        1,
        0xe1580bf28fcba2e5,
        0x70c0b15becfd1c52,
        0x9ae1c92c2c5615c0,
        0x8c8045c7a63b54a4,
        0x93f16a594aa1524a,
    ),
    (
        "solar_only",
        2,
        0xfbdc4eb8a339bb5a,
        0xa33c282146c7f55a,
        0x8b06b98ce815c03d,
        0xa442acd00e4195a6,
        0xa9c949ec11c0bc2e,
    ),
    (
        "solar_only",
        3,
        0x425737f8e58e723c,
        0x573ddf42ceeb327a,
        0xc7148c69b70fca1c,
        0x04df801c1b3872a5,
        0x8a833e0fb479ed68,
    ),
    (
        "nuclear_heavy",
        1,
        0xe1580bf28fcba2e5,
        0xad45a7ed6ad12fb5,
        0xf137a408a1eca4ef,
        0xb7e4cbf8e13ce9e6,
        0x48a371ee97a5ca8b,
    ),
    (
        "nuclear_heavy",
        2,
        0xfbdc4eb8a339bb5a,
        0xa04969ea51c63a0d,
        0x880796f493c85c81,
        0xb2693d8e60ee7cb9,
        0xf47c3250b710ea55,
    ),
    (
        "nuclear_heavy",
        3,
        0x425737f8e58e723c,
        0xb6660a55d2a7c3ad,
        0x3b21a7d6934d3b6c,
        0x9471951a2b98ca34,
        0x20d041b4ac6f6618,
    ),
];

/// The `conjunction` variant on Small, as week 9 left it, in the same form
/// as [`WEEK6`]. Week 9 moved the blackout's start from 0.2–1.0 sols into
/// the run to 0.01–0.03 sols, so that it overlaps every preset's build; the
/// terrain and the scenario are unchanged, and so are the draws from the
/// `comms` stream, but every run's link events and outcomes moved with the
/// blackout.
#[allow(clippy::type_complexity)]
const WEEK9: [(&str, u64, u64, u64, u64, u64, u64); 3] = [
    (
        "conjunction",
        1,
        0xe1580bf28fcba2e5,
        0xacfd5b2623089397,
        0x451bb53705442365,
        0x3bf9e687e63cc19e,
        0x9b35f4f94b29aff8,
    ),
    (
        "conjunction",
        2,
        0xfbdc4eb8a339bb5a,
        0x3ea091df5f37cfff,
        0x6f871a2c5319ead6,
        0x2032fa336160071b,
        0xeb49e5b3f02926b6,
    ),
    (
        "conjunction",
        3,
        0x425737f8e58e723c,
        0x196f3a7c7d429cdf,
        0x0b4151fa259a0a74,
        0x3bfcf8a59c8d3092,
        0x316785e8d4bd7ff9,
    ),
];

/// (true_site, scenario under its week-6 name, everything after `run_start`) of a raw log.
fn week6_fingerprints(text: &str) -> (u64, u64, u64) {
    let (first, rest) = text.split_at(text.find('\n').unwrap() + 1);
    // Versions belong to `run_start` alone, so none reaches a fingerprint.
    for key in [
        "\"protocol_version\"",
        "\"simulator\":{",
        "\"coordinator\":{",
    ] {
        assert!(!rest.contains(key), "{key} after run_start");
    }
    let first = first.trim_end();
    let site =
        &first[first.find("\"true_site\":").unwrap() + "\"true_site\":".len()..first.len() - 1];
    let scenario = week6_scenario(between(first, "\"scenario\":", ",\"true_site\":"));
    (fnv1a(site), fnv1a(&scenario), fnv1a(rest))
}

/// Check a variant's Small runs against a table of fingerprints.
fn check_runs(table: &[(&str, u64, u64, u64, u64, u64, u64)], variant: &str) {
    let v: Variant = variant.parse().unwrap();
    let mut checked = 0;
    for &(name, seed, site, scenario, parker_run, idle_run, builder_run) in table {
        if name != variant {
            continue;
        }
        let params = Params::scenario(Preset::Small, v);
        let parker = Parker {
            capacity: Vec::new(),
            next: 0,
        };
        let (_, parker_log, _) = support::run_raw(parker, seed, &params);
        let (_, idle_log, _) = support::run_raw(support::Idle, seed, &params);
        let (_, builder_log, _) = support::run_raw(support::builder::Builder::new(), seed, &params);
        let runs = [
            ("parker", parker_log, parker_run),
            ("idle", idle_log, idle_run),
            ("builder", builder_log, builder_run),
        ];
        for (agent, log, run) in runs {
            let (s, sc, rest) = week6_fingerprints(&log);
            assert_eq!(s, site, "{variant} seed {seed} ({agent}): terrain");
            assert_eq!(sc, scenario, "{variant} seed {seed} ({agent}): scenario");
            assert_eq!(rest, run, "{variant} seed {seed}: {agent} run");
        }
        checked += 1;
    }
    assert!(checked > 0, "no fingerprints for {variant}");
}

#[test]
fn small_nominal_keeps_its_week_6_worlds_and_runs() {
    assert_eq!(
        Params::default(),
        Params::scenario(Preset::Small, Variant::Nominal)
    );
    check_runs(&WEEK6, "nominal");
}

#[test]
fn the_other_week_6_variants_keep_their_small_runs() {
    for variant in ["cascading_faults", "solar_only", "nuclear_heavy"] {
        check_runs(&WEEK6, variant);
    }
}

#[test]
fn the_conjunction_keeps_its_week_9_small_runs() {
    check_runs(&WEEK9, "conjunction");
}

fn strip_agent<A>((report, text, _): (RunReport, String, A)) -> (RunReport, String) {
    (report, text)
}

/// The raw text of every `comms` event in a log, with its time.
fn comms_events(text: &str) -> Vec<(f64, &str)> {
    text.lines()
        .filter(|l| {
            l.contains("\"kind\":\"observed\",\"event\":{") && l.contains(",\"type\":\"comms\",")
        })
        .map(|l| {
            let event = &l[l.find("\"event\":").unwrap() + "\"event\":".len()..l.len() - 1];
            let t: serde_json::Value = serde_json::from_str(event).unwrap();
            (t["t"].as_f64().unwrap(), event)
        })
        .collect()
}

#[test]
fn the_link_schedule_does_not_depend_on_the_coordinator() {
    for params in [Params::default(), Params::conjunction()] {
        for seed in 1..=5 {
            // A fleet kept alive for 60 sols, one that dies of cold in about
            // 17, and one that builds the pad and talks to Earth.
            let parker = Parker {
                capacity: Vec::new(),
                next: 0,
            };
            let runs = [
                strip_agent(support::run_raw(parker, seed, &params)),
                strip_agent(support::run_raw(support::Idle, seed, &params)),
                strip_agent(support::run_raw(
                    support::builder::Builder::new(),
                    seed,
                    &params,
                )),
            ];
            let until = runs
                .iter()
                .map(|(r, _)| r.end_t)
                .fold(f64::INFINITY, f64::min);
            let link: Vec<Vec<&str>> = runs
                .iter()
                .map(|(_, text)| {
                    comms_events(text)
                        .into_iter()
                        .filter(|&(t, _)| t < until)
                        .map(|(_, e)| e)
                        .collect()
                })
                .collect();
            assert!(!link[0].is_empty());
            assert_eq!(link[0], link[1], "seed {seed}: parker vs idle");
            assert_eq!(link[0], link[2], "seed {seed}: parker vs builder");
            if params.conjunction {
                let parker_all = comms_events(&runs[0].1);
                assert!(
                    parker_all
                        .iter()
                        .any(|(_, e)| e.contains("\"cause\":\"conjunction\"")),
                    "seed {seed}: the blackout is reported"
                );
            }
        }
    }
}

#[test]
fn a_60_sol_run_is_fast() {
    // A fleet kept alive to the time limit, around a conjunction: every
    // sol of the run is simulated, link and all.
    let started = std::time::Instant::now();
    let (report, _, _) = support::run_raw(
        Parker {
            capacity: Vec::new(),
            next: 0,
        },
        3,
        &Params::conjunction(),
    );
    let elapsed = started.elapsed();
    assert_eq!(report.end_reason, EndReason::TimeLimit);
    eprintln!("60-sol run: {elapsed:?}");
    let bound = if cfg!(debug_assertions) { 30.0 } else { 1.0 };
    assert!(elapsed.as_secs_f64() < bound, "{elapsed:?}");
}
