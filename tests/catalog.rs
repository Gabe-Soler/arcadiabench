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

#[test]
fn catalog_1_1_says_what_makes_it_harder() {
    let c = Catalog::V1_1;
    for seed in [1, 9] {
        let mut starts = Vec::new();
        for preset in Preset::ALL {
            let h = hello(seed, &Params::scenario(c, preset, Variant::Nominal));
            let s = &h.scenario;
            // A hundred sols, from a seeded time of the sol.
            let limit_sols = (s.time_limit_t - s.start_t) / s.sol_length_s;
            assert!((limit_sols - 100.0).abs() < 1e-9, "{limit_sols}");
            assert!(s.start_t > 0.0 && s.start_t < s.sol_length_s);
            starts.push(s.start_t);
            // Earth takes hours to answer, so a level-3 request waits a sol.
            let k = s.constants.as_ref().expect("constants");
            assert_eq!(s.policy.level3.max_wait_s, s.sol_length_s);
            assert!(k.earth_response_mean_s >= 4.0 * 3600.0);
            assert!(k.earth_response_mean_s < s.policy.level3.max_wait_s);
            // Compaction takes most of the tolerance off a cell.
            assert_eq!(k.compaction_settlement_m, 0.04);
            // Six, twelve and twenty robots, half of them sinterers.
            let sinterers = s
                .robots
                .iter()
                .filter(|r| r.capability(CapabilityName::Sinter).is_some())
                .count();
            assert_eq!(2 * sinterers, s.robots.len(), "{preset}");
            assert!(s.power.fission_w >= 40_000.0);
        }
        // The start depends on the seed alone, never on the preset.
        assert!(starts.windows(2).all(|w| w[0] == w[1]), "{starts:?}");
    }
    // Different seeds start at different times of the sol.
    let start = |seed| hello(seed, &Params::scenario(c, Preset::Small, Variant::Nominal));
    assert_ne!(start(1).scenario.start_t, start(2).scenario.start_t);
    // The power variants are other plants, and the stresses are switched on.
    let p = |v| Params::scenario(c, Preset::Small, v);
    let nominal = p(Variant::Nominal);
    assert!(nominal.pass_windows && !nominal.dust_storm && !nominal.conjunction);
    assert!(p(Variant::DustStorm).dust_storm);
    assert!(p(Variant::Conjunction).conjunction);
    assert_eq!(p(Variant::SolarOnly).fission_w, 0.0);
    assert!(p(Variant::SolarOnly).solar_peak_w > nominal.solar_peak_w);
    assert!(p(Variant::NuclearHeavy).fission_w > nominal.fission_w);
    assert!(p(Variant::CascadingFaults).random_fault_per_h > nominal.random_fault_per_h);
    for v in Variant::ALL {
        assert_eq!((p(v).catalog, p(v).variant), (c, v));
    }
}

#[test]
fn catalog_1_1_runs_to_its_end_with_the_null_coordinator() {
    let out = support::scratch_dir("catalog-1.1-null");
    let opts = Options {
        jobs: std::thread::available_parallelism().map_or(4, |n| n.get()),
        out: Some(out.clone()),
        save_logs: true,
    };
    let connect = bench::command(vec![NULL.into()], Duration::from_secs(10));
    let stresses = [Variant::Nominal, Variant::DustStorm, Variant::Conjunction];
    let result = bench::suite(
        &connect,
        Catalog::V1_1,
        &Preset::ALL,
        &stresses,
        &[3],
        &opts,
        &|_, _| true,
    )
    .unwrap();
    assert_eq!(result.cells.len(), 9);
    for cell in &result.cells {
        let r = &cell.runs[0];
        let what = format!("{}/{}", cell.preset, cell.variant);
        assert!(
            matches!(r.reason, EndReason::FleetLost | EndReason::TimeLimit),
            "{what}: {:?}",
            r.reason
        );
        assert_eq!((r.metrics.commands, r.score), (0, 0.0), "{what}");
        assert_eq!(r.error, None);
        let log = bench::cell_dir(&out, cell.catalog, cell.preset, cell.variant)
            .join("logs")
            .join(format!("{}.jsonl.gz", r.run_id));
        support::assert_valid_log(&support::gunzip(&log));
        let c = &r.metrics.conditions;
        // The link runs in pass windows, so it is mostly down between them.
        let between = c.between_passes_s.expect("a 1.1 run records it");
        assert!(between > 0.5 * r.metrics.sols * SOL_S, "{what}: {between}");
        match cell.variant {
            Variant::DustStorm => assert!(c.storm_s > 10.0 * SOL_S, "{what}: {}", c.storm_s),
            Variant::Conjunction => {
                assert!(c.blackout_s > 13.0 * SOL_S, "{what}: {}", c.blackout_s)
            }
            _ => assert_eq!(c.blackout_s, 0.0, "{what}"),
        }
    }
    // The storm and the blackout fall when the seed says, whatever the
    // preset: idle crews of every size meet the same conditions.
    for variant in stresses {
        let met: Vec<(f64, f64)> = result
            .cells
            .iter()
            .filter(|c| c.variant == variant)
            .map(|c| {
                let k = &c.runs[0].metrics.conditions;
                (k.storm_s, k.blackout_s)
            })
            .collect();
        assert!(met.windows(2).all(|w| w[0] == w[1]), "{variant}: {met:?}");
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
