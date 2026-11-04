//! Scoring (`docs/scoring.md` gives the formulae and the reasoning).
//!
//! - **A run** is scored from its `run_end.metrics` ([`RunRecord`]). A
//!   completed run's *speed* is its scenario's reference build time over the
//!   time it took, capped at 1; its *score* is that speed, or 0 if the pad
//!   was not finished.
//! - **A scorecard** ([`Scorecard`]) aggregates the runs of one coordinator
//!   on one scenario over seeds. Its headline is success rate × mean speed,
//!   which is the mean score of its runs. Beside it are completion, quality,
//!   energy efficiency, fleet health, autonomy and the conditions the runs met.
//! - **Resilience** ([`Resilience`]) compares a coordinator's headlines under
//!   the stress variants with its own nominal headline, preset by preset.
//!
//! `schemas/scorecard.schema.json` describes the JSON of all three.

pub mod logs;
mod render;

use crate::sim::params::{Preset, Variant};
use arcadia_protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// `schemas/scorecard.schema.json`: the JSON Schema (draft 2020-12) of
/// scorecards, run records and reports. It refers to arcadia-protocol's
/// schemas for run metrics and software info.
pub const SCHEMA: &str = include_str!("../../schemas/scorecard.schema.json");

/// One run as the bench records it: a line of `metrics.jsonl`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub scenario_id: String,
    /// The preset, when the scenario is in the catalog.
    pub preset: Option<Preset>,
    /// The variant, when the scenario is in the catalog.
    pub variant: Option<Variant>,
    pub seed: u64,
    pub simulator: SoftwareInfo,
    pub coordinator: SoftwareInfo,
    pub reason: EndReason,
    /// What the coordinator did wrong, when it failed mid-run. Event logs
    /// do not record it, so runs scored from logs never have one.
    pub error: Option<String>,
    /// The scenario's reference build time, sols ([`reference_sols`]).
    pub reference_sols: Option<f64>,
    /// [`speed`] of a completed run; `None` otherwise.
    pub speed: Option<f64>,
    /// The run's contribution to the headline: its speed if it completed, else 0.
    pub score: f64,
    pub metrics: RunMetrics,
}

impl RunRecord {
    /// Score a finished run.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scenario: &Scenario,
        pad: &LandingPadSpec,
        seed: u64,
        simulator: SoftwareInfo,
        coordinator: SoftwareInfo,
        reason: EndReason,
        error: Option<String>,
        metrics: RunMetrics,
    ) -> Self {
        let (preset, variant) = parse_scenario_id(&scenario.scenario_id).unzip();
        let reference = reference_sols(scenario, pad);
        let speed = match (metrics.completed, reference) {
            (true, Some(r)) => Some(speed(metrics.sols, r)),
            _ => None,
        };
        Self {
            run_id: format!("{}-{seed}", scenario.scenario_id),
            scenario_id: scenario.scenario_id.clone(),
            preset,
            variant,
            seed,
            simulator,
            coordinator,
            reason,
            error,
            reference_sols: reference,
            speed,
            score: speed.unwrap_or(0.0),
            metrics,
        }
    }
}

/// The preset and variant named by a catalog scenario id,
/// `landing-pad-<preset>-<variant>`.
pub fn parse_scenario_id(id: &str) -> Option<(Preset, Variant)> {
    let (preset, variant) = id.strip_prefix("landing-pad-")?.split_once('-')?;
    Some((preset.parse().ok()?, variant.parse().ok()?))
}

/// The per-area treatments every pad cell needs before the pad is done:
/// grading, compaction, sintering when the pad is stabilized, and a
/// verification scan.
fn finishing(pad: &LandingPadSpec) -> Vec<CapabilityName> {
    use CapabilityName::*;
    let mut needed = vec![Grade, Compact, Scan];
    if pad.stabilize {
        needed.push(Sinter);
    }
    needed
}

/// A scenario's reference build time, in sols: how long its crew would need
/// to give the whole pad its slowest per-area treatment, with every robot
/// that can do it working at its nominal rate at once, and no travel,
/// charging, waiting or wear. For every preset that treatment is sintering,
/// so the reference is the pad's area over the crew's total sinter rate.
/// No real build can beat it by more than duration noise, and it shrinks in
/// proportion as a preset adds sinterers.
///
/// `None` if the crew lacks a treatment the pad needs: then no run can finish.
pub fn reference_sols(scenario: &Scenario, pad: &LandingPadSpec) -> Option<f64> {
    let side = |lo: u32, hi: u32| f64::from(hi.saturating_sub(lo)) + 1.0;
    let cells = side(pad.area.min.x, pad.area.max.x) * side(pad.area.min.y, pad.area.max.y);
    let area_m2 = cells * scenario.site.cell_size_m * scenario.site.cell_size_m;
    let mut slowest_s: f64 = 0.0;
    for c in finishing(pad) {
        let rate: f64 = scenario
            .robots
            .iter()
            .filter_map(|r| r.capability(c))
            .map(|cap| cap.rate)
            .fold(0.0, |a, b| a + b);
        if rate <= 0.0 {
            return None;
        }
        slowest_s = slowest_s.max(area_m2 / rate);
    }
    (scenario.sol_length_s > 0.0).then(|| slowest_s / scenario.sol_length_s)
}

/// The speed of a run that finished the pad in `sols`: the reference build
/// time over the time taken, capped at 1. In (0, 1], and twice as fast is
/// twice the speed until the cap.
pub fn speed(sols: f64, reference_sols: f64) -> f64 {
    if sols <= reference_sols {
        1.0
    } else {
        reference_sols / sols
    }
}

/// The runs of one coordinator on one scenario, aggregated over seeds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scorecard {
    pub scenario_id: String,
    pub preset: Option<Preset>,
    pub variant: Option<Variant>,
    pub coordinator: SoftwareInfo,
    pub simulator: SoftwareInfo,
    /// The seeds scored, ascending.
    pub seeds: Vec<u64>,
    /// The scenario's reference build time, sols.
    pub reference_sols: Option<f64>,
    /// Success rate × mean speed of the completed runs: the mean score of
    /// all runs. In [0, 1].
    pub headline: f64,
    pub completion: Completion,
    pub quality: Quality,
    pub energy: Energy,
    pub fleet: Fleet,
    pub autonomy: Autonomy,
    pub conditions: Conditions,
}

/// Whether the pad was finished, and how fast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Completion {
    pub runs: u64,
    pub completed: u64,
    /// Completed runs over runs.
    pub success_rate: f64,
    /// Mean speed of the completed runs.
    pub mean_speed: Option<f64>,
    /// Sols taken by the completed runs: mean, median, fastest and slowest.
    pub mean_sols: Option<f64>,
    pub median_sols: Option<f64>,
    pub min_sols: Option<f64>,
    pub max_sols: Option<f64>,
    pub end_reasons: EndReasons,
}

/// How many runs ended each way.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EndReasons {
    pub goals_complete: u64,
    pub time_limit: u64,
    pub fleet_lost: u64,
    pub coordinator_error: u64,
    pub simulator_error: u64,
}

/// The finished pad against its design.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quality {
    /// Mean share of pad cells in spec at the end, over all runs.
    pub mean_cells_in_spec: f64,
    /// Share of runs whose whole pad was verified at the end.
    pub verified_rate: f64,
    /// Share of runs whose berm was complete at the end.
    pub berm_complete_rate: f64,
    /// Mean RMS elevation error of the completed pads, m.
    pub mean_rms_error_m: Option<f64>,
    /// Largest elevation error of any completed pad, m.
    pub max_abs_error_m: Option<f64>,
    /// Steepest slope on any completed pad, degrees.
    pub max_slope_deg: Option<f64>,
}

/// Energy used, and the closest approach to a power deficit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Energy {
    /// Energy drawn from robot batteries in all runs, over the pads
    /// completed, Wh: unfinished runs count against it.
    pub energy_per_pad_wh: Option<f64>,
    pub mean_robot_energy_wh: f64,
    pub mean_grid_energy_wh: f64,
    /// The lowest site storage of any run, as a fraction of capacity.
    pub min_storage_fraction: f64,
    pub mean_min_storage_fraction: f64,
    /// Mean time per run with the grid short of demand, s.
    pub mean_deficit_s: f64,
}

/// Robot time and robots lost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fleet {
    pub mean_faults: f64,
    pub mean_robot_hours_lost: f64,
    pub mean_robots_lost: f64,
    /// Robots lost over all runs.
    pub robots_lost: u64,
}

/// How much the crew leaned on Earth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Autonomy {
    /// Approval requests sent to Earth, per run.
    pub mean_approval_requests: f64,
    /// Messages of any kind sent to Earth, per run.
    pub mean_downlink_messages: f64,
    pub mean_notifications: f64,
    pub mean_status_reports: f64,
    /// Time spent waiting for Earth's answers, per run, s.
    pub mean_approval_wait_s: f64,
    pub mean_policy_violations: f64,
}

/// The adversity the runs met, per run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conditions {
    pub mean_storm_s: f64,
    pub mean_blackout_s: f64,
    pub mean_outage_s: f64,
}

/// Why runs could not be scored together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreError {
    /// There were no runs to score.
    NoRuns,
    /// Runs that belong to different scorecards were mixed.
    Mixed(String),
    /// The same seed was scored twice.
    DuplicateSeed { scenario_id: String, seed: u64 },
}

impl fmt::Display for ScoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScoreError::NoRuns => write!(f, "no runs to score"),
            ScoreError::Mixed(what) => write!(f, "cannot score these runs together: {what}"),
            ScoreError::DuplicateSeed { scenario_id, seed } => {
                write!(f, "seed {seed} of {scenario_id} appears twice")
            }
        }
    }
}

impl std::error::Error for ScoreError {}

/// Mean of `xs`; `None` when empty. Summed in order from +0.0, so the result
/// never depends on anything but the values and their order.
fn mean(xs: impl IntoIterator<Item = f64>) -> Option<f64> {
    let (sum, n) = xs.into_iter().fold((0.0, 0u64), |(s, n), x| (s + x, n + 1));
    (n > 0).then(|| sum / n as f64)
}

fn median(mut xs: Vec<f64>) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(f64::total_cmp);
    let mid = xs.len() / 2;
    Some(if xs.len() % 2 == 1 {
        xs[mid]
    } else {
        (xs[mid - 1] + xs[mid]) / 2.0
    })
}

fn share(n: usize, of: usize) -> f64 {
    n as f64 / of as f64
}

impl Scorecard {
    /// Aggregate the runs of one coordinator on one scenario. The order of
    /// `runs` does not matter.
    pub fn from_runs(runs: &[RunRecord]) -> Result<Self, ScoreError> {
        let first = runs.first().ok_or(ScoreError::NoRuns)?;
        for r in runs {
            let same = r.scenario_id == first.scenario_id
                && r.coordinator == first.coordinator
                && r.simulator == first.simulator
                && r.reference_sols == first.reference_sols;
            if !same {
                return Err(ScoreError::Mixed(format!(
                    "{} by {} {} on {} {} and {} by {} {} on {} {}",
                    first.scenario_id,
                    first.coordinator.name,
                    first.coordinator.version,
                    first.simulator.name,
                    first.simulator.version,
                    r.scenario_id,
                    r.coordinator.name,
                    r.coordinator.version,
                    r.simulator.name,
                    r.simulator.version,
                )));
            }
        }
        let mut runs: Vec<&RunRecord> = runs.iter().collect();
        runs.sort_by_key(|r| r.seed);
        if let Some(w) = runs.windows(2).find(|w| w[0].seed == w[1].seed) {
            return Err(ScoreError::DuplicateSeed {
                scenario_id: first.scenario_id.clone(),
                seed: w[0].seed,
            });
        }
        let n = runs.len();
        let done: Vec<&RunRecord> = runs
            .iter()
            .copied()
            .filter(|r| r.metrics.completed)
            .collect();
        let all =
            |f: fn(&RunMetrics) -> f64| mean(runs.iter().map(|r| f(&r.metrics))).unwrap_or(0.0);
        let finished = |f: fn(&RunMetrics) -> f64| mean(done.iter().map(|r| f(&r.metrics)));
        let worst = |f: fn(&RunMetrics) -> f64| done.iter().map(|r| f(&r.metrics)).reduce(f64::max);
        let sols: Vec<f64> = done.iter().map(|r| r.metrics.sols).collect();

        let mut reasons = EndReasons::default();
        for r in &runs {
            *match r.reason {
                EndReason::GoalsComplete => &mut reasons.goals_complete,
                EndReason::TimeLimit => &mut reasons.time_limit,
                EndReason::FleetLost => &mut reasons.fleet_lost,
                EndReason::CoordinatorError => &mut reasons.coordinator_error,
                EndReason::SimulatorError => &mut reasons.simulator_error,
            } += 1;
        }
        let robot_energy: f64 = runs
            .iter()
            .map(|r| r.metrics.energy.robot_energy_wh)
            .fold(0.0, |a, b| a + b);

        Ok(Scorecard {
            scenario_id: first.scenario_id.clone(),
            preset: first.preset,
            variant: first.variant,
            coordinator: first.coordinator.clone(),
            simulator: first.simulator.clone(),
            seeds: runs.iter().map(|r| r.seed).collect(),
            reference_sols: first.reference_sols,
            headline: mean(runs.iter().map(|r| r.score)).unwrap_or(0.0),
            completion: Completion {
                runs: n as u64,
                completed: done.len() as u64,
                success_rate: share(done.len(), n),
                mean_speed: mean(done.iter().filter_map(|r| r.speed)),
                mean_sols: mean(sols.iter().copied()),
                median_sols: median(sols.clone()),
                min_sols: sols.iter().copied().reduce(f64::min),
                max_sols: sols.iter().copied().reduce(f64::max),
                end_reasons: reasons,
            },
            quality: Quality {
                mean_cells_in_spec: all(|m| {
                    if m.quality.cells_total == 0 {
                        0.0
                    } else {
                        m.quality.cells_in_spec as f64 / m.quality.cells_total as f64
                    }
                }),
                verified_rate: share(
                    runs.iter().filter(|r| r.metrics.quality.verified).count(),
                    n,
                ),
                berm_complete_rate: share(
                    runs.iter()
                        .filter(|r| r.metrics.quality.berm_complete)
                        .count(),
                    n,
                ),
                mean_rms_error_m: finished(|m| m.quality.rms_error_m),
                max_abs_error_m: worst(|m| m.quality.max_abs_error_m),
                max_slope_deg: worst(|m| m.quality.max_slope_deg),
            },
            energy: Energy {
                energy_per_pad_wh: (!done.is_empty()).then(|| robot_energy / done.len() as f64),
                mean_robot_energy_wh: all(|m| m.energy.robot_energy_wh),
                mean_grid_energy_wh: all(|m| m.energy.grid_energy_wh),
                min_storage_fraction: runs
                    .iter()
                    .map(|r| r.metrics.energy.min_storage_fraction)
                    .fold(f64::INFINITY, f64::min),
                mean_min_storage_fraction: all(|m| m.energy.min_storage_fraction),
                mean_deficit_s: all(|m| m.energy.deficit_s),
            },
            fleet: Fleet {
                mean_faults: all(|m| m.fleet.faults as f64),
                mean_robot_hours_lost: all(|m| m.fleet.robot_hours_lost),
                mean_robots_lost: all(|m| m.fleet.robots_lost as f64),
                robots_lost: runs.iter().map(|r| r.metrics.fleet.robots_lost).sum(),
            },
            autonomy: Autonomy {
                mean_approval_requests: all(|m| m.autonomy.approval_requests as f64),
                mean_downlink_messages: all(|m| m.autonomy.downlink_messages as f64),
                mean_notifications: all(|m| m.autonomy.notifications as f64),
                mean_status_reports: all(|m| m.autonomy.status_reports as f64),
                mean_approval_wait_s: all(|m| m.autonomy.approval_wait_s),
                mean_policy_violations: all(|m| m.autonomy.policy_violations as f64),
            },
            conditions: Conditions {
                mean_storm_s: all(|m| m.conditions.storm_s),
                mean_blackout_s: all(|m| m.conditions.blackout_s),
                mean_outage_s: all(|m| m.conditions.outage_s),
            },
        })
    }

    /// A readable summary, several lines long.
    pub fn render(&self) -> String {
        render::scorecard(self)
    }
}

/// A coordinator's resilience: how much of its nominal headline it keeps
/// under the stress variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resilience {
    pub coordinator: SoftwareInfo,
    /// Mean of the presets' resilience, over the presets where it is known.
    pub resilience: Option<f64>,
    pub presets: Vec<PresetResilience>,
}

/// Resilience on one preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetResilience {
    pub preset: Preset,
    pub nominal_headline: f64,
    /// Each stress variant's headline over the nominal headline, capped at
    /// 1; `None` if that variant was not scored or the nominal headline is 0.
    pub dust_storm: Option<f64>,
    pub conjunction: Option<f64>,
    pub cascading_faults: Option<f64>,
    /// Mean of the three; `None` unless all three are known.
    pub resilience: Option<f64>,
}

/// Scorecards for every coordinator and scenario among some runs, with the
/// resilience of every coordinator that has nominal scorecards. This is what
/// `arcadiabench score` and `arcadiabench suite` produce.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// Sorted by coordinator, then preset and variant in catalog order.
    pub scorecards: Vec<Scorecard>,
    /// Sorted by coordinator.
    pub resilience: Vec<Resilience>,
}

type CoordinatorKey = (String, String);

fn coordinator_key(c: &SoftwareInfo) -> CoordinatorKey {
    (c.name.clone(), c.version.clone())
}

/// Catalog scenarios first, in catalog order, then the rest by id.
fn scenario_order(s: &Scorecard) -> (u8, Option<Preset>, Option<Variant>, String) {
    (
        u8::from(s.preset.is_none() || s.variant.is_none()),
        s.preset,
        s.variant,
        s.scenario_id.clone(),
    )
}

impl Report {
    /// Group runs into scorecards and work out resilience.
    pub fn from_runs(runs: &[RunRecord]) -> Result<Self, ScoreError> {
        if runs.is_empty() {
            return Err(ScoreError::NoRuns);
        }
        let mut groups: BTreeMap<(CoordinatorKey, String), Vec<RunRecord>> = BTreeMap::new();
        for r in runs {
            groups
                .entry((coordinator_key(&r.coordinator), r.scenario_id.clone()))
                .or_default()
                .push(r.clone());
        }
        let mut scorecards = groups
            .values()
            .map(|g| Scorecard::from_runs(g))
            .collect::<Result<Vec<_>, _>>()?;
        scorecards.sort_by(|a, b| {
            (coordinator_key(&a.coordinator), scenario_order(a))
                .cmp(&(coordinator_key(&b.coordinator), scenario_order(b)))
        });
        let resilience = resilience(&scorecards);
        Ok(Report {
            scorecards,
            resilience,
        })
    }

    /// The scorecards side by side, then resilience: a readable table.
    pub fn render(&self) -> String {
        render::report(self)
    }
}

/// The resilience of every coordinator with at least one nominal scorecard.
pub fn resilience(scorecards: &[Scorecard]) -> Vec<Resilience> {
    let mut by_coordinator: BTreeMap<CoordinatorKey, Vec<&Scorecard>> = BTreeMap::new();
    for s in scorecards {
        by_coordinator
            .entry(coordinator_key(&s.coordinator))
            .or_default()
            .push(s);
    }
    let mut out = Vec::new();
    for cards in by_coordinator.values() {
        let headline = |p: Preset, v: Variant| {
            cards
                .iter()
                .find(|s| s.preset == Some(p) && s.variant == Some(v))
                .map(|s| s.headline)
        };
        let presets: Vec<PresetResilience> = Preset::ALL
            .into_iter()
            .filter_map(|p| {
                let nominal = headline(p, Variant::Nominal)?;
                let kept = |v: Variant| {
                    let h = headline(p, v)?;
                    (nominal > 0.0).then(|| (h / nominal).min(1.0))
                };
                let (dust_storm, conjunction, cascading_faults) = (
                    kept(Variant::DustStorm),
                    kept(Variant::Conjunction),
                    kept(Variant::CascadingFaults),
                );
                let resilience = match (dust_storm, conjunction, cascading_faults) {
                    (Some(a), Some(b), Some(c)) => Some((a + b + c) / 3.0),
                    _ => None,
                };
                Some(PresetResilience {
                    preset: p,
                    nominal_headline: nominal,
                    dust_storm,
                    conjunction,
                    cascading_faults,
                    resilience,
                })
            })
            .collect();
        if presets.is_empty() {
            continue;
        }
        out.push(Resilience {
            coordinator: cards[0].coordinator.clone(),
            resilience: mean(presets.iter().filter_map(|p| p.resilience)),
            presets,
        });
    }
    out
}

#[cfg(test)]
mod tests;
