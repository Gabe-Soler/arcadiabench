//! One run: scenario → handshake → observe/decide loop → end, with every step
//! written to the event log. Transport-independent protocol rules (version
//! compatibility, decision seq) are enforced here.
//!
//! The log never goes back in time (semantics §8.6): Earth's messages are
//! logged when Earth sends them, between observations, and every event is
//! logged at the time it was delivered to the coordinator, while keeping the
//! time it happened.

use crate::coordinator::{BenchError, CoordinatorLink};
use crate::log::EventLog;
use crate::scenario;
use crate::sim::params::Params;
use crate::sim::{Advance, Sim};
use crate::software_info;
use arcadia_protocol::*;
use std::io::Write;

#[derive(Debug, Clone, PartialEq)]
pub struct RunReport {
    pub coordinator: SoftwareInfo,
    pub end_reason: EndReason,
    pub end_t: SimTime,
    pub decisions: u64,
    pub commands: usize,
    /// Fraction of the pad meeting spec at the end.
    pub progress: f64,
    /// The metrics written on `run_end`.
    pub metrics: RunMetrics,
    /// Lost-link returns: idle robots out of coverage driving back on their own.
    pub lost_link_returns: u64,
}

pub fn run_session<L: CoordinatorLink + ?Sized, W: Write>(
    link: &mut L,
    seed: u64,
    params: &Params,
    log: &mut EventLog<W>,
) -> Result<RunReport, BenchError> {
    let generated = scenario::generate(seed, params);
    let true_site = generated
        .terrain
        .to_site_map("arcadia-pad-site", "Arcadia Planitia landing pad site");
    let scenario = generated.scenario.clone();
    let run_id = format!("{}-{seed}", scenario.scenario_id);

    let welcome = link.start(Hello {
        protocol_version: PROTOCOL_VERSION.into(),
        simulator: software_info(),
        run_id: run_id.clone(),
        seed,
        scenario: scenario.clone(),
    })?;
    if !is_compatible(&welcome.protocol_version, PROTOCOL_VERSION) {
        let message = format!(
            "coordinator speaks {}, arcadiabench speaks {PROTOCOL_VERSION}",
            welcome.protocol_version
        );
        link.abort(ErrorCode::VersionMismatch, &message);
        return Err(BenchError::Protocol(message));
    }

    let mut sim = Sim::new(generated, seed, params.clone());
    log.record(
        sim.now(),
        LogEntry::RunStart {
            protocol_version: PROTOCOL_VERSION.into(),
            simulator: software_info(),
            coordinator: Some(welcome.coordinator.clone()),
            run_id,
            seed,
            scenario: Box::new(scenario),
            true_site: Box::new(true_site),
        },
    )?;

    let mut report = RunReport {
        coordinator: welcome.coordinator,
        end_reason: EndReason::SimulatorError,
        end_t: 0.0,
        decisions: 0,
        commands: 0,
        progress: 0.0,
        metrics: RunMetrics::default(),
        lost_link_returns: 0,
    };
    let result = drive(link, &mut sim, log, &mut report);
    let reason = match &result {
        Ok(reason) => *reason,
        Err(_) => EndReason::CoordinatorError,
    };
    // The final instant: what Earth sent since the last observation, events
    // the coordinator never saw, then the ground truth at the end, then
    // run_end. The log always ends this way, even when the coordinator
    // misbehaved; the coordinator itself only gets `end`.
    log_uplinks(&mut sim, log)?;
    let end_t = sim.now();
    for event in sim.take_undelivered() {
        log.record(end_t, LogEntry::Observed { event })?;
    }
    log.record(
        sim.now(),
        LogEntry::Truth {
            robots: sim.truth_robots(),
            changed_cells: sim.take_changed_cells(),
            site: sim.site_truth(),
        },
    )?;
    let metrics = sim.metrics(reason);
    report.end_reason = reason;
    report.end_t = sim.now();
    report.progress = sim.progress();
    report.metrics = metrics.clone();
    report.lost_link_returns = sim.lost_link_returns();
    log.record(sim.now(), LogEntry::RunEnd { reason, metrics })?;
    result.map(|_| report)
}

fn drive<L: CoordinatorLink + ?Sized, W: Write>(
    link: &mut L,
    sim: &mut Sim,
    log: &mut EventLog<W>,
    report: &mut RunReport,
) -> Result<EndReason, BenchError> {
    loop {
        match sim.advance() {
            Advance::Observe(obs) => {
                log_uplinks(sim, log)?;
                for event in &obs.events {
                    log.record(
                        obs.t,
                        LogEntry::Observed {
                            event: event.clone(),
                        },
                    )?;
                }
                log.record(
                    obs.t,
                    LogEntry::Truth {
                        robots: sim.truth_robots(),
                        changed_cells: sim.take_changed_cells(),
                        site: sim.site_truth(),
                    },
                )?;
                let seq = obs.seq;
                let decision = link.decide(obs)?;
                if decision.seq != seq {
                    let message = format!(
                        "decision seq {} does not match observation seq {seq}",
                        decision.seq
                    );
                    link.abort(ErrorCode::UnexpectedMessage, &message);
                    return Err(BenchError::Protocol(message));
                }
                report.decisions += 1;
                report.commands += decision.commands.len();
                for command in &decision.commands {
                    log.record(
                        sim.now(),
                        LogEntry::Commanded {
                            command: command.clone(),
                        },
                    )?;
                }
                for (message, deliver_t) in sim.apply(&decision) {
                    log.record(sim.now(), LogEntry::EarthDownlink { deliver_t, message })?;
                }
            }
            Advance::End { t, reason } => {
                link.finish(End { t, reason })?;
                return Ok(reason);
            }
        }
    }
}

/// Log what Earth has sent since the last call, at the times it sent it.
fn log_uplinks<W: Write>(sim: &mut Sim, log: &mut EventLog<W>) -> Result<(), BenchError> {
    for (t, deliver_t, message) in sim.take_uplinks() {
        log.record(t, LogEntry::EarthUplink { deliver_t, message })?;
    }
    Ok(())
}
