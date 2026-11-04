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

/// A record of `coordinator` on Small under `variant`, from the real scenario.
fn record(coordinator: &str, variant: Variant, seed: u64, completed: bool, sols: f64) -> RunRecord {
    let p = Params::scenario(Preset::Small, variant);
    let g = scenario::generate(seed, &p);
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
        let g = scenario::generate(1, &Params::scenario(preset, Variant::Nominal));
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
    assert_eq!(presets::crew(Preset::Large).len(), 20);
}

#[test]
fn scenario_ids_name_their_preset_and_variant() {
    assert_eq!(
        parse_scenario_id("landing-pad-medium-cascading_faults"),
        Some((Preset::Medium, Variant::CascadingFaults))
    );
    assert_eq!(parse_scenario_id("landing-pad-small"), None);
    assert_eq!(parse_scenario_id("landing-pad-huge-nominal"), None);
    let r = record("x", Variant::DustStorm, 7, true, 1.0);
    assert_eq!(r.run_id, "landing-pad-small-dust_storm-7");
    assert_eq!(
        (r.preset, r.variant),
        (Some(Preset::Small), Some(Variant::DustStorm))
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
    assert_eq!(p.preset, Preset::Small);
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
fn seed_lists_collapse_into_ranges() {
    assert_eq!(render::seed_ranges(&[1, 2, 3, 5, 7, 8]), "1-3, 5, 7-8");
    assert_eq!(render::seed_ranges(&[4]), "4");
    assert_eq!(render::seed_ranges(&[]), "");
}
