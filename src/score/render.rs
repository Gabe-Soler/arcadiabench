//! Readable tables of scorecards and resilience, for the command line.

use super::{Report, Scorecard};
use std::fmt::Write;

/// `1-5, 8, 10-12`.
pub(super) fn seed_ranges(seeds: &[u64]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < seeds.len() {
        let mut j = i;
        while j + 1 < seeds.len() && seeds[j + 1] == seeds[j] + 1 {
            j += 1;
        }
        parts.push(if i == j {
            seeds[i].to_string()
        } else {
            format!("{}-{}", seeds[i], seeds[j])
        });
        i = j + 1;
    }
    parts.join(", ")
}

fn opt(x: Option<f64>, digits: usize) -> String {
    x.map_or_else(|| "-".into(), |v| format!("{v:.digits$}"))
}

fn scaled(x: Option<f64>, k: f64, digits: usize) -> String {
    opt(x.map(|v| v * k), digits)
}

fn pct(x: f64) -> String {
    format!("{:.1}%", x * 100.0)
}

pub(super) fn scorecard(s: &Scorecard) -> String {
    let c = &s.completion;
    let q = &s.quality;
    let e = &s.energy;
    let f = &s.fleet;
    let a = &s.autonomy;
    let k = &s.conditions;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {} on {} ({} runs, seeds {})",
        s.coordinator.name,
        s.coordinator.version,
        s.scenario_id,
        c.runs,
        seed_ranges(&s.seeds)
    );
    let _ = writeln!(
        out,
        "  headline       {:.3} = success {} x mean speed {} (reference {} sols)",
        s.headline,
        pct(c.success_rate),
        opt(c.mean_speed, 3),
        opt(s.reference_sols, 3)
    );
    let r = &c.end_reasons;
    let reasons: Vec<String> = [
        ("goals_complete", r.goals_complete),
        ("time_limit", r.time_limit),
        ("fleet_lost", r.fleet_lost),
        ("coordinator_error", r.coordinator_error),
        ("simulator_error", r.simulator_error),
    ]
    .into_iter()
    .filter(|&(_, n)| n > 0)
    .map(|(name, n)| format!("{name} {n}"))
    .collect();
    let _ = writeln!(
        out,
        "  completion     {}/{} complete | sols mean {}, median {}, range {} to {} | {}",
        c.completed,
        c.runs,
        opt(c.mean_sols, 3),
        opt(c.median_sols, 3),
        opt(c.min_sols, 3),
        opt(c.max_sols, 3),
        reasons.join(", ")
    );
    let _ = writeln!(
        out,
        "  quality        pad in spec {} | rms error {} mm, worst error {} mm, steepest slope {} deg | verified {}, berm {}",
        pct(q.mean_cells_in_spec),
        scaled(q.mean_rms_error_m, 1000.0, 1),
        scaled(q.max_abs_error_m, 1000.0, 1),
        opt(q.max_slope_deg, 2),
        pct(q.verified_rate),
        pct(q.berm_complete_rate)
    );
    let _ = writeln!(
        out,
        "  energy         {} kWh per pad | robot {:.1} kWh, grid {:.1} kWh per run | lowest storage {} (mean {}) | deficit {:.2} h per run",
        scaled(e.energy_per_pad_wh, 0.001, 1),
        e.mean_robot_energy_wh / 1000.0,
        e.mean_grid_energy_wh / 1000.0,
        pct(e.min_storage_fraction),
        pct(e.mean_min_storage_fraction),
        e.mean_deficit_s / 3600.0
    );
    let _ = writeln!(
        out,
        "  fleet health   {:.2} faults, {:.2} robot-hours lost, {:.2} robots lost per run ({} lost in all)",
        f.mean_faults, f.mean_robot_hours_lost, f.mean_robots_lost, f.robots_lost
    );
    let _ = writeln!(
        out,
        "  autonomy       {:.2} approval requests, {:.2} messages to Earth, {:.2} h waiting for approvals, {:.2} policy violations per run",
        a.mean_approval_requests,
        a.mean_downlink_messages,
        a.mean_approval_wait_s / 3600.0,
        a.mean_policy_violations
    );
    let _ = writeln!(
        out,
        "  conditions     storm {:.1} h, blackout {:.1} h, outage {:.1} h per run",
        k.mean_storm_s / 3600.0,
        k.mean_blackout_s / 3600.0,
        k.mean_outage_s / 3600.0
    );
    out
}

/// Left-aligned first columns, right-aligned numbers.
fn table(header: &[&str], rows: &[Vec<String>], left: usize) -> String {
    let widths: Vec<usize> = (0..header.len())
        .map(|i| {
            rows.iter()
                .map(|r| r[i].chars().count())
                .chain([header[i].chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for row in std::iter::once(header.iter().map(|h| h.to_string()).collect::<Vec<_>>())
        .chain(rows.iter().cloned())
    {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                if i < left {
                    format!("{cell:<w$}", w = widths[i])
                } else {
                    format!("{cell:>w$}", w = widths[i])
                }
            })
            .collect();
        let _ = writeln!(out, "{}", cells.join("  ").trim_end());
    }
    out
}

pub(super) fn report(r: &Report) -> String {
    let mut out = String::new();
    if let [only] = r.scorecards.as_slice() {
        out.push_str(&scorecard(only));
    } else {
        let rows: Vec<Vec<String>> = r
            .scorecards
            .iter()
            .map(|s| {
                let c = &s.completion;
                vec![
                    format!("{} {}", s.coordinator.name, s.coordinator.version),
                    s.scenario_id.clone(),
                    c.runs.to_string(),
                    format!("{:.3}", s.headline),
                    pct(c.success_rate),
                    opt(c.mean_speed, 3),
                    opt(c.mean_sols, 3),
                    opt(c.median_sols, 3),
                    scaled(s.quality.mean_rms_error_m, 1000.0, 1),
                    scaled(s.energy.energy_per_pad_wh, 0.001, 1),
                    pct(s.energy.min_storage_fraction),
                    format!("{:.2}", s.fleet.mean_robot_hours_lost),
                    s.fleet.robots_lost.to_string(),
                    format!("{:.2}", s.autonomy.mean_approval_requests),
                    format!("{:.2}", s.autonomy.mean_approval_wait_s / 3600.0),
                    format!("{:.2}", s.autonomy.mean_policy_violations),
                ]
            })
            .collect();
        out.push_str(&table(
            &[
                "coordinator",
                "scenario",
                "runs",
                "headline",
                "success",
                "speed",
                "mean sols",
                "median sols",
                "rms mm",
                "kWh/pad",
                "min storage",
                "robot-h lost",
                "lost",
                "requests",
                "wait h",
                "violations",
            ],
            &rows,
            2,
        ));
    }
    if !r.resilience.is_empty() {
        let mut rows = Vec::new();
        for c in &r.resilience {
            let who = format!("{} {}", c.coordinator.name, c.coordinator.version);
            for p in &c.presets {
                rows.push(vec![
                    who.clone(),
                    p.preset.name().to_string(),
                    format!("{:.3}", p.nominal_headline),
                    opt(p.dust_storm, 3),
                    opt(p.conjunction, 3),
                    opt(p.cascading_faults, 3),
                    opt(p.resilience, 3),
                ]);
            }
            rows.push(vec![
                who,
                "all".into(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                opt(c.resilience, 3),
            ]);
        }
        let _ = writeln!(
            out,
            "\nresilience: each stress variant's headline over the nominal headline, capped at 1"
        );
        out.push_str(&table(
            &[
                "coordinator",
                "preset",
                "nominal",
                "dust_storm",
                "conjunction",
                "cascading_faults",
                "resilience",
            ],
            &rows,
            2,
        ));
    }
    out
}
