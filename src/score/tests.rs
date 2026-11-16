use super::*;
use crate::presets;
use crate::scenario;
use crate::sim::params::Params;

fn info(name: &str) -> SoftwareInfo {
    SoftwareInfo {
        name: name.into(),
        version: "1.0.0".into(),
    }
}

fn metrics(completed: bool, sols: f64) -> RunMetrics {
    let mut m = RunMetrics {
        completed,
        sols,
        ..RunMetrics::default()
    };
    m.quality.cells_total = 400;
    m.quality.cells_in_spec = if completed { 400 } else { 100 };
    m.quality.verified = completed;
    m.quality.berm_complete = completed;
    m.quality.rms_error_m = if completed { 0.01 } else { 0.2 };
    m.quality.max_abs_error_m = if completed { 0.03 } else { 0.5 };
    m.quality.max_slope_deg = if completed { 1.0 } else { 9.0 };
    m.energy.robot_energy_wh = 30_000.0;
    m.energy.min_storage_fraction = 0.5;
    m
}

/// A record of `coordinator` on catalog 1.0's Small under `variant`, from
/// the real scenario.
fn record(coordinator: &str, variant: Variant, seed: u64, completed: bool, sols: f64) -> RunRecord {
    record_on(Catalog::V1_0, coordinator, variant, seed, completed, sols)
}

fn record_on(
    catalog: Catalog,
    coordinator: &str,
    variant: Variant,
    seed: u64,
    completed: bool,
    sols: f64,
) -> RunRecord {
    let p = Params::scenario(catalog, Preset::Small, variant);
    record_of(&p, coordinator, seed, completed, sols)
}

/// A record of `coordinator` on the scenario `p` describes.
fn record_of(p: &Params, coordinator: &str, seed: u64, completed: bool, sols: f64) -> RunRecord {
    let g = scenario::generate(seed, p);
    let Uplink::Goal {
        objective: Objective::LandingPad { pad },
        ..
    } = &g.goal.body
    else {
        panic!("a landing pad");
    };
    let reason = if completed {
        EndReason::GoalsComplete
    } else {
        EndReason::TimeLimit
    };
    RunRecord::new(
        &g.scenario,
        pad,
        seed,
        info("arcadiabench"),
        info(coordinator),
        reason,
        None,
        metrics(completed, sols),
    )
}

#[test]
fn speed_is_bounded_monotonic_and_scale_free() {
    assert_eq!(speed(0.2, 0.5), 1.0);
    assert_eq!(speed(0.5, 0.5), 1.0);
    assert!((speed(1.0, 0.5) - 0.5).abs() < 1e-12);
    let mut last = 1.0;
    for i in 1..=600 {
        let s = speed(0.5 + f64::from(i) * 0.1, 0.5);
        assert!(s > 0.0 && s < last);
        last = s;
    }
    // Twice as slow is half the speed, whatever the scale.
    for r in [0.05, 0.45, 3.0] {
        assert!((speed(4.0 * r, r) / speed(2.0 * r, r) - 0.5).abs() < 1e-12);
    }
}

#[test]
fn the_reference_is_the_crews_sintering_time() {
    for (preset, sinterers) in [
        (Preset::Small, 1.0),
        (Preset::Medium, 4.0),
        (Preset::Large, 8.0),
    ] {
        let p = Params::scenario(Catalog::V1_0, preset, Variant::Nominal);
        let g = scenario::generate(1, &p);
        let Uplink::Goal {
            objective: Objective::LandingPad { pad },
            ..
        } = &g.goal.body
        else {
            panic!();
        };
        let r = reference_sols(&g.scenario, pad).unwrap();
        let expected = 400.0 / (0.01 * sinterers) / 88_775.0;
        assert!((r - expected).abs() < 1e-12, "{preset}: {r}");
        // Without sinterers no run can finish, so there is no reference.
        let mut s = g.scenario.clone();
        s.robots
            .retain(|r| r.capability(CapabilityName::Sinter).is_none());
        assert_eq!(reference_sols(&s, pad), None);
    }
    assert_eq!(presets::crew(Catalog::V1_0, Preset::Large).len(), 20);
}

#[test]
fn records_name_their_catalog_preset_and_variant() {
    let r = record("x", Variant::DustStorm, 7, true, 1.0);
    assert_eq!(r.run_id, "landing-pad-small-dust_storm-7");
    assert_eq!(
        (r.catalog, r.preset, r.variant),
        (
            Some(Catalog::V1_0),
            Some(Preset::Small),
            Some(Variant::DustStorm)
        )
    );
    let r = record_on(Catalog::V1_1, "x", Variant::DustStorm, 7, true, 1.0);
    assert_eq!(r.run_id, "landing-pad-1.1-small-dust_storm-7");
    assert_eq!(
        (r.catalog, r.preset, r.variant),
        (
            Some(Catalog::V1_1),
            Some(Preset::Small),
            Some(Variant::DustStorm)
        )
    );
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["catalog"], "1.1");
    assert_eq!(serde_json::from_value::<RunRecord>(json).unwrap(), r);
}

#[test]
fn files_from_before_1_1_name_no_catalog_and_are_catalog_1_0s() {
    // A record, a scorecard and a report as ArcadiaBench 1.0 wrote them.
    let strip = |mut v: serde_json::Value| {
        v.as_object_mut().unwrap().remove("catalog").unwrap();
        v
    };
    let runs: Vec<RunRecord> = Variant::ALL
        .into_iter()
        .map(|v| {
            let old = strip(serde_json::to_value(record("x", v, 1, true, 1.0)).unwrap());
            serde_json::from_value(old).unwrap()
        })
        .collect();
    assert!(runs.iter().all(|r| r.catalog.is_none()));
    let report = Report::from_runs(&runs).unwrap();
    assert!(
        report
            .scorecards
            .iter()
            .all(|s| s.catalog == Some(Catalog::V1_0))
    );
    assert_eq!(report.resilience.len(), 1);
    assert_eq!(report.resilience[0].catalog, Catalog::V1_0);

    let mut old = serde_json::to_value(&report).unwrap();
    for part in ["scorecards", "resilience"] {
        for item in old[part].as_array_mut().unwrap() {
            *item = strip(item.take());
        }
    }
    let read: Report = serde_json::from_value(old).unwrap();
    assert!(read.scorecards.iter().all(|s| s.catalog.is_none()));
    assert_eq!(read.resilience, report.resilience);
    // Scored again, the old scorecards land where the new ones do.
    assert_eq!(resilience(&read.scorecards), report.resilience);
    // A scenario outside every catalog has none.
    assert_eq!(catalog_of(None, "my-own-scenario"), None);
    assert_eq!(
        catalog_of(Some(Catalog::V1_1), "my-own-scenario"),
        Some(Catalog::V1_1)
    );
}

#[test]
fn the_headline_is_success_rate_times_mean_speed() {
    let runs = vec![
        record("x", Variant::Nominal, 3, true, 0.9),
        record("x", Variant::Nominal, 1, true, 0.6),
        record("x", Variant::Nominal, 2, false, 60.0),
        record("x", Variant::Nominal, 4, true, 2.0),
    ];
    let s = Scorecard::from_runs(&runs).unwrap();
    let c = &s.completion;
    assert_eq!(s.seeds, [1, 2, 3, 4]);
    assert_eq!((c.runs, c.completed), (4, 3));
    assert_eq!(c.success_rate, 0.75);
    let r = s.reference_sols.unwrap();
    let mean_speed = (r / 0.9 + r / 0.6 + r / 2.0) / 3.0;
    assert!((c.mean_speed.unwrap() - mean_speed).abs() < 1e-12);
    assert!((s.headline - 0.75 * mean_speed).abs() < 1e-12);
    assert_eq!(c.median_sols, Some(0.9));
    assert_eq!((c.min_sols, c.max_sols), (Some(0.6), Some(2.0)));
    assert!((c.mean_sols.unwrap() - 3.5 / 3.0).abs() < 1e-12);
    assert_eq!(
        (c.end_reasons.goals_complete, c.end_reasons.time_limit),
        (3, 1)
    );
    // Quality of the finished pads; progress over all runs.
    assert_eq!(s.quality.mean_rms_error_m, Some(0.01));
    assert_eq!(s.quality.max_slope_deg, Some(1.0));
    assert!((s.quality.mean_cells_in_spec - (3.0 + 0.25) / 4.0).abs() < 1e-12);
    assert_eq!(s.quality.verified_rate, 0.75);
    // The unfinished run's energy counts against the finished pads.
    assert_eq!(s.energy.energy_per_pad_wh, Some(4.0 * 30_000.0 / 3.0));
    // The order of the runs does not matter, down to the last bit.
    let mut reversed = runs.clone();
    reversed.reverse();
    assert_eq!(Scorecard::from_runs(&reversed).unwrap(), s);
}

#[test]
fn a_scorecard_with_no_finished_pad_scores_zero() {
    let runs = vec![
        record("x", Variant::Nominal, 1, false, 60.0),
        record("x", Variant::Nominal, 2, false, 17.0),
    ];
    let s = Scorecard::from_runs(&runs).unwrap();
    assert_eq!(s.headline, 0.0);
    assert_eq!(s.completion.mean_speed, None);
    assert_eq!(s.completion.median_sols, None);
    assert_eq!(s.energy.energy_per_pad_wh, None);
    assert_eq!(s.quality.mean_rms_error_m, None);
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"mean_speed\":null"), "{json}");
}

#[test]
fn scorecards_refuse_mixed_or_repeated_runs() {
    assert_eq!(Scorecard::from_runs(&[]), Err(ScoreError::NoRuns));
    let a = record("x", Variant::Nominal, 1, true, 1.0);
    assert!(matches!(
        Scorecard::from_runs(&[a.clone(), record("y", Variant::Nominal, 2, true, 1.0)]),
        Err(ScoreError::Mixed(_))
    ));
    assert!(matches!(
        Scorecard::from_runs(&[a.clone(), record("x", Variant::DustStorm, 2, true, 1.0)]),
        Err(ScoreError::Mixed(_))
    ));
    assert_eq!(
        Scorecard::from_runs(&[a.clone(), a]),
        Err(ScoreError::DuplicateSeed {
            scenario_id: "landing-pad-small-nominal".into(),
            seed: 1
        })
    );
}

#[test]
fn resilience_compares_stress_headlines_with_nominal() {
    let mut runs = Vec::new();
    for seed in 1..=4 {
        runs.push(record("x", Variant::Nominal, seed, true, 1.0));
        // Half as fast in a storm, all lost in a conjunction, faster under faults.
        runs.push(record("x", Variant::DustStorm, seed, true, 2.0));
        runs.push(record("x", Variant::Conjunction, seed, false, 60.0));
        runs.push(record("x", Variant::CascadingFaults, seed, true, 0.8));
        runs.push(record("y", Variant::Nominal, seed, true, 1.0));
        runs.push(record("y", Variant::DustStorm, seed, seed % 2 == 0, 1.0));
    }
    let report = Report::from_runs(&runs).unwrap();
    let ids: Vec<(&str, &str)> = report
        .scorecards
        .iter()
        .map(|s| (s.coordinator.name.as_str(), s.scenario_id.as_str()))
        .collect();
    assert_eq!(
        ids,
        [
            ("x", "landing-pad-small-nominal"),
            ("x", "landing-pad-small-dust_storm"),
            ("x", "landing-pad-small-conjunction"),
            ("x", "landing-pad-small-cascading_faults"),
            ("y", "landing-pad-small-nominal"),
            ("y", "landing-pad-small-dust_storm"),
        ]
    );
    let [x, y] = report.resilience.as_slice() else {
        panic!("{:?}", report.resilience);
    };
    let p = &x.presets[0];
    assert_eq!(p.preset, Some(Preset::Small));
    assert!((p.dust_storm.unwrap() - 0.5).abs() < 1e-12);
    assert_eq!(p.conjunction, Some(0.0));
    assert_eq!(p.cascading_faults, Some(1.0), "capped at 1");
    assert!((p.resilience.unwrap() - 0.5).abs() < 1e-12);
    assert!((x.resilience.unwrap() - 0.5).abs() < 1e-12);
    // Without every stress variant, resilience stays unknown.
    assert!((y.presets[0].dust_storm.unwrap() - 0.5).abs() < 1e-12);
    assert_eq!(y.presets[0].resilience, None);
    assert_eq!(y.resilience, None);
    let text = report.render();
    assert!(text.contains("resilience"), "{text}");
}

#[test]
fn resilience_never_compares_across_catalogs() {
    let mut runs = Vec::new();
    for seed in 1..=2 {
        // Catalog 1.0: nominal and every stress variant at full speed.
        for v in [Variant::Nominal].into_iter().chain(Variant::STRESS) {
            runs.push(record("x", v, seed, true, 1.0));
        }
        // Catalog 1.1, where a build takes weeks: half as fast as its own
        // nominal in a storm, and the other stress variants not run.
        runs.push(record_on(
            Catalog::V1_1,
            "x",
            Variant::Nominal,
            seed,
            true,
            45.0,
        ));
        runs.push(record_on(
            Catalog::V1_1,
            "x",
            Variant::DustStorm,
            seed,
            true,
            90.0,
        ));
    }
    let report = Report::from_runs(&runs).unwrap();
    // Catalog by catalog, each in its own order.
    let ids: Vec<&str> = report
        .scorecards
        .iter()
        .map(|s| s.scenario_id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "landing-pad-small-nominal",
            "landing-pad-small-dust_storm",
            "landing-pad-small-conjunction",
            "landing-pad-small-cascading_faults",
            "landing-pad-1.1-small-nominal",
            "landing-pad-1.1-small-dust_storm",
        ]
    );
    let [old, new] = report.resilience.as_slice() else {
        panic!("one resilience per catalog: {:?}", report.resilience);
    };
    assert_eq!((old.catalog, new.catalog), (Catalog::V1_0, Catalog::V1_1));
    assert_eq!(old.coordinator, new.coordinator);
    assert_eq!(old.presets[0].dust_storm, Some(1.0));
    assert_eq!(old.resilience, Some(1.0));
    // Its storm is set against its own nominal, not catalog 1.0's.
    assert!((new.presets[0].dust_storm.unwrap() - 0.5).abs() < 1e-12);
    assert_ne!(
        new.presets[0].nominal_headline,
        old.presets[0].nominal_headline
    );
    assert_eq!(new.presets[0].conjunction, None);
    assert_eq!(new.resilience, None);
    let text = report.render();
    assert!(text.contains("catalog"), "{text}");
}

#[test]
fn seed_lists_collapse_into_ranges() {
    assert_eq!(render::seed_ranges(&[1, 2, 3, 5, 7, 8]), "1-3, 5, 7-8");
    assert_eq!(render::seed_ranges(&[4]), "4");
    assert_eq!(render::seed_ranges(&[]), "");
}

#[test]
fn every_fleet_crew_is_scored_against_one_reference() {
    let mix = |sc, ex, hl, cp| CrewMix {
        scout: sc,
        excavator: ex,
        hauler: hl,
        compactor_sinterer: cp,
    };
    let (few, many) = (mix(1, 1, 1, 2), mix(1, 1, 1, 8));
    let c = Catalog::V1_1;
    let run = |crew, v, sols| record_of(&Params::fleet(c, crew, v), "x", 1, true, sols);
    let (a, b) = (
        run(few, Variant::Nominal, 80.0),
        run(many, Variant::Nominal, 20.0),
    );
    assert_eq!(a.run_id, "landing-pad-1.1-crew.1.1.1.2-nominal-1");
    assert_eq!((a.catalog, a.preset, a.crew), (Some(c), None, Some(few)));
    assert_eq!(a.variant, Some(Variant::Nominal));
    // Eleven sinterers fit in catalog 1.1's 10 t: 400 m² at 11 x 4e-5 m²/s.
    let reference = 400.0 / (11.0 * 4e-5) / 88_775.0;
    assert!((a.reference_sols.unwrap() - reference).abs() < 1e-9);
    assert_eq!(a.reference_sols, b.reference_sols);
    // Measured against itself, the crew of two sinterers would have a
    // reference five and a half times longer, and a higher speed for it.
    let own = {
        let g = scenario::generate(1, &Params::fleet(c, few, Variant::Nominal));
        reference_sols(&g.scenario, &g.mission.pad).unwrap()
    };
    assert!((own / reference - 5.5).abs() < 1e-9, "{own}");
    assert!((a.speed.unwrap() - reference / 80.0).abs() < 1e-12);
    assert!((b.speed.unwrap() - reference / 20.0).abs() < 1e-12);
    // The Standard track still measures a preset against its own crew.
    let small = record_on(c, "x", Variant::Nominal, 1, true, 80.0);
    assert!((small.reference_sols.unwrap() - 400.0 / (3.0 * 4e-5) / 88_775.0).abs() < 1e-9);
    assert_eq!((small.preset, small.crew), (Some(Preset::Small), None));
    // Catalog 1.0 has its own budget and its own designs.
    let old = record_of(
        &Params::fleet(Catalog::V1_0, few, Variant::Nominal),
        "x",
        1,
        true,
        1.0,
    );
    assert_eq!(old.run_id, "landing-pad-crew.1.1.1.2-nominal-1");
    assert!((old.reference_sols.unwrap() - 400.0 / (12.0 * 0.01) / 88_775.0).abs() < 1e-12);

    // A crew's record and scorecard carry it; a preset's leave it out.
    let json = serde_json::to_value(&a).unwrap();
    assert_eq!(json["crew"]["compactor_sinterer"], 2);
    assert!(json["preset"].is_null());
    assert_eq!(serde_json::from_value::<RunRecord>(json).unwrap(), a);
    assert!(serde_json::to_value(&small).unwrap().get("crew").is_none());
    let card = Scorecard::from_runs(std::slice::from_ref(&a)).unwrap();
    assert_eq!((card.crew, card.preset), (Some(few), None));
    assert_eq!(card.reference_sols, a.reference_sols);
}

#[test]
fn each_fleet_crew_has_its_own_resilience() {
    let mix = |cp| CrewMix {
        scout: 1,
        excavator: 1,
        hauler: 1,
        compactor_sinterer: cp,
    };
    let c = Catalog::V1_1;
    let mut runs = Vec::new();
    for seed in 1..=2 {
        // The Standard track, at full strength under every stress.
        for v in [Variant::Nominal].into_iter().chain(Variant::STRESS) {
            runs.push(record_on(c, "x", v, seed, true, 60.0));
        }
        // One crew keeps half its pace in a storm; another keeps all of it
        // but was not run under cascading faults.
        for (crew, storm) in [(mix(4), 80.0), (mix(8), 40.0)] {
            let run = |v, sols| record_of(&Params::fleet(c, crew, v), "x", seed, true, sols);
            runs.push(run(Variant::Nominal, 40.0));
            runs.push(run(Variant::DustStorm, storm));
            runs.push(run(Variant::Conjunction, 40.0));
            if crew == mix(4) {
                runs.push(run(Variant::CascadingFaults, 40.0));
            }
        }
    }
    let report = Report::from_runs(&runs).unwrap();
    // Presets first, then the crews, each in variant order.
    let ids: Vec<&str> = report
        .scorecards
        .iter()
        .map(|s| s.scenario_id.strip_prefix("landing-pad-1.1-").unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "small-nominal",
            "small-dust_storm",
            "small-conjunction",
            "small-cascading_faults",
            "crew.1.1.1.4-nominal",
            "crew.1.1.1.4-dust_storm",
            "crew.1.1.1.4-conjunction",
            "crew.1.1.1.4-cascading_faults",
            "crew.1.1.1.8-nominal",
            "crew.1.1.1.8-dust_storm",
            "crew.1.1.1.8-conjunction",
        ]
    );
    let [standard, four, eight] = report.resilience.as_slice() else {
        panic!("one resilience per line-up: {:?}", report.resilience);
    };
    assert_eq!((standard.crew, standard.resilience), (None, Some(1.0)));
    assert_eq!(standard.presets[0].preset, Some(Preset::Small));
    assert_eq!((four.crew, four.catalog), (Some(mix(4)), c));
    let [line] = four.presets.as_slice() else {
        panic!("a crew is one line-up");
    };
    assert_eq!(line.preset, None);
    assert!((line.dust_storm.unwrap() - 0.5).abs() < 1e-12);
    assert_eq!(
        (line.conjunction, line.cascading_faults),
        (Some(1.0), Some(1.0))
    );
    assert!((four.resilience.unwrap() - 2.5 / 3.0).abs() < 1e-12);
    // Without every stress variant a crew's resilience stays unknown.
    assert_eq!((eight.crew, eight.resilience), (Some(mix(8)), None));
    assert_eq!(eight.presets[0].dust_storm, Some(1.0));
    let json = serde_json::to_value(&report).unwrap();
    assert!(json["resilience"][0].get("crew").is_none());
    assert_eq!(json["resilience"][1]["crew"]["compactor_sinterer"], 4);
    assert!(json["resilience"][1]["presets"][0]["preset"].is_null());
    assert_eq!(serde_json::from_value::<Report>(json).unwrap(), report);
    let text = report.render();
    assert!(text.contains("crew 1.1.1.4"), "{text}");
}
