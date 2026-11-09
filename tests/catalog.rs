//! The scenario catalogs: in each, every preset with every variant says a
//! valid hello; catalog 1.0's scenarios run to their end with valid logs,
//! and every preset is buildable.

mod support;

use arcadia_protocol::validate::validator;
use arcadia_protocol::*;
use arcadiabench::bench::{self, Options};
use arcadiabench::presets;
use arcadiabench::session::hello;
use arcadiabench::sim::params::{Catalog, Params, Preset, Variant};
use std::time::Duration;

const NULL: &str = env!("CARGO_BIN_EXE_null-coordinator");
const SOL_S: f64 = 88_775.0;

#[test]
fn every_scenario_says_a_valid_hello() {
    let v = validator("messages.schema.json", "SimToCoordinator").unwrap();
    for catalog in Catalog::ALL {
        for preset in Preset::ALL {
            for variant in Variant::ALL {
                for seed in [0, 7, u64::MAX] {
                    let p = Params::scenario(catalog, preset, variant);
                    let h = hello(seed, &p);
                    let json =
                        serde_json::to_value(SimToCoordinator::Hello(Box::new(h.clone()))).unwrap();
                    if let Err(e) = v.validate(&json) {
                        panic!(
                            "{catalog} {preset}/{variant} seed {seed}: {e} at {}",
                            e.instance_path()
                        );
                    }
                    let id = match catalog {
                        Catalog::V1_0 => format!("landing-pad-{preset}-{variant}"),
                        Catalog::V1_1 => format!("landing-pad-1.1-{preset}-{variant}"),
                    };
                    assert_eq!(h.scenario.scenario_id, id);
                    assert_eq!(h.run_id, format!("{id}-{seed}"));
                    assert_eq!(h.scenario.robots, presets::crew(catalog, preset));
                    assert_eq!(h.scenario.stations, presets::stations(catalog, preset));
                    assert_eq!(h.scenario.depots, presets::depots(preset));
                    assert_eq!(h.scenario.power.mode, p.power_mode);
                    assert_eq!(h.scenario.power.fission_w, p.fission_w);
                }
            }
        }
    }
}

/// A catalog 1.1 log under catalog 1.0's names: only the scenario id, and
/// the run id made from it, tell the two apart.
fn renamed_1_0(text: &str) -> String {
    assert!(text.contains("\"scenario_id\":\"landing-pad-1.1-"));
    text.replace("landing-pad-1.1-", "landing-pad-")
}

#[test]
fn catalog_1_1_still_carries_catalog_1_0() {
    // Until catalog 1.1 gets its own values, a run on it is catalog 1.0's
    // run byte for byte, apart from the id. Every scenario's world and
    // weather with an idle crew, and whole builds on a few.
    for preset in Preset::ALL {
        for variant in Variant::ALL {
            let old = Params::scenario(Catalog::V1_0, preset, variant);
            let new = Params::scenario(Catalog::V1_1, preset, variant);
            assert_eq!(new.catalog, Catalog::V1_1);
            assert_eq!(
                Params {
                    catalog: Catalog::V1_0,
                    ..new.clone()
                },
                old
            );
            let (_, a, _) = support::run_raw(support::Idle, 4, &old);
            let (_, b, _) = support::run_raw(support::Idle, 4, &new);
            assert!(a == renamed_1_0(&b), "{preset}/{variant}: idle runs differ");
        }
    }
    for (preset, variant, seed) in [
        (Preset::Small, Variant::Nominal, 1),
        (Preset::Small, Variant::CascadingFaults, 2),
        (Preset::Medium, Variant::DustStorm, 3),
        (Preset::Large, Variant::Conjunction, 4),
    ] {
        let run = |catalog| {
            let p = Params::scenario(catalog, preset, variant);
            let (report, text, _) = support::run_raw(support::builder::Builder::new(), seed, &p);
            (report.end_reason, report.metrics, text)
        };
        let (old_reason, old_metrics, old) = run(Catalog::V1_0);
        let (new_reason, new_metrics, new) = run(Catalog::V1_1);
        assert_eq!(old_reason, new_reason, "{preset}/{variant}");
        assert_eq!(old_metrics, new_metrics, "{preset}/{variant}");
        assert!(
            old == renamed_1_0(&new),
            "{preset}/{variant}: builds differ"
        );
    }
}

#[test]
fn every_scenario_runs_to_its_end_with_the_null_coordinator() {
    let out = support::scratch_dir("catalog-null");
    let opts = Options {
        jobs: std::thread::available_parallelism().map_or(4, |n| n.get()),
        out: Some(out.clone()),
        save_logs: true,
    };
    let connect = bench::command(vec![NULL.into()], Duration::from_secs(10));
    let result = bench::suite(
        &connect,
        Catalog::V1_0,
        &Preset::ALL,
        &Variant::ALL,
        &[3],
        &opts,
        &|_, _| true,
    )
    .unwrap();
    assert_eq!(result.cells.len(), 18);
    for cell in &result.cells {
        let r = &cell.runs[0];
        let what = format!("{}/{}", cell.preset, cell.variant);
        // Idle robots drain on cold nights until nothing can act again.
        assert_eq!(r.reason, EndReason::FleetLost, "{what}");
        assert!(
            r.metrics.sols > 10.0 && r.metrics.sols < 60.0,
            "{what}: {}",
            r.metrics.sols
        );
        assert_eq!((r.metrics.commands, r.score), (0, 0.0), "{what}");
        assert_eq!(r.error, None);
        assert_eq!(cell.catalog, Catalog::V1_0);
        let log = bench::cell_dir(&out, cell.catalog, cell.preset, cell.variant)
            .join("logs")
            .join(format!("{}.jsonl.gz", r.run_id));
        support::assert_valid_log(&support::gunzip(&log));
        // What each variant brings shows in the run's conditions.
        let c = &r.metrics.conditions;
        match cell.variant {
            Variant::DustStorm => assert!(c.storm_s > 3.0 * SOL_S, "{what}: {}", c.storm_s),
            Variant::Conjunction => assert!(c.blackout_s > 9.0 * SOL_S, "{what}: {}", c.blackout_s),
            _ => assert_eq!(c.blackout_s, 0.0, "{what}"),
        }
    }
    // The same world for every preset: idle crews see the same weather and link.
    for variant in Variant::ALL {
        let conditions: Vec<_> = result
            .cells
            .iter()
            .filter(|c| c.variant == variant)
            .map(|c| c.runs[0].metrics.conditions.storm_s)
            .collect();
        assert!(
            conditions.windows(2).all(|w| w[0] == w[1]),
            "{variant}: {conditions:?}"
        );
    }
}

#[test]
fn the_dust_storm_arrives_early_and_hard() {
    for seed in 1..=10 {
        let (_, records, _) = support::run(support::Idle, seed, &Params::dust_storm());
        let storm = records.iter().find_map(|r| match &r.entry {
            LogEntry::Observed { event } => match event.kind {
                EventKind::Weather {
                    condition: WeatherCondition::DustStorm,
                    ..
                } => Some(event.t),
                _ => None,
            },
            _ => None,
        });
        let start = storm.expect("a storm is reported");
        assert!(start <= 0.25 * SOL_S, "seed {seed}: {start}");
        let peak_tau = records
            .iter()
            .filter_map(|r| match &r.entry {
                LogEntry::Truth { site, .. } if r.t < 2.0 * SOL_S => Some(site.optical_depth),
                _ => None,
            })
            .fold(0.0, f64::max);
        assert!(peak_tau > 3.0, "seed {seed}: {peak_tau}");
    }
}

#[test]
fn the_scripted_builder_finishes_every_preset() {
    // It only knows sc-1, ex-1, hl-1 and cp-1, which every preset has.
    for preset in [Preset::Medium, Preset::Large] {
        let p = Params::scenario(Catalog::V1_0, preset, Variant::Nominal);
        let (report, text, _) = support::run_raw(support::builder::Builder::new(), 1, &p);
        assert_eq!(report.end_reason, EndReason::GoalsComplete, "{preset}");
        assert!(
            report.metrics.sols < 3.0,
            "{preset}: {}",
            report.metrics.sols
        );
        support::assert_valid_log(&text);
    }
}
