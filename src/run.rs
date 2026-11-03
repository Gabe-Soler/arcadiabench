//! One benchmark run, end to end.

use crate::coordinator::{BenchError, CoordinatorProcess};
use crate::log::EventLog;
use crate::{scenario, software_info};
use arcadia_protocol::*;
use std::io::Write;

#[derive(Debug, PartialEq)]
pub struct RunReport {
    pub coordinator: SoftwareInfo,
    pub decisions: u64,
    pub commands: usize,
}

/// Week-1 placeholder run: handshake, one observation at t = 0 (comms up plus
/// the landing-pad goal), one decision, then `end`. There is no physics yet,
/// so commands are logged but not executed.
pub fn run_placeholder<W: Write>(
    program: &str,
    args: &[String],
    seed: u64,
    log: &mut EventLog<W>,
) -> Result<RunReport, BenchError> {
    let scenario = scenario::placeholder();
    let run_id = format!("placeholder-{seed}");
    let hello = Hello {
        protocol_version: PROTOCOL_VERSION.into(),
        simulator: software_info(),
        run_id: run_id.clone(),
        seed,
        scenario: scenario.clone(),
    };

    let mut coordinator = CoordinatorProcess::launch(program, args, hello)?;
    let t = scenario.start_t;
    log.record(
        t,
        LogEntry::RunStart {
            protocol_version: PROTOCOL_VERSION.into(),
            simulator: software_info(),
            coordinator: Some(coordinator.info.clone()),
            run_id,
            seed,
            true_site: Box::new(scenario.site.clone()),
            scenario: Box::new(scenario.clone()),
        },
    )?;

    let events = vec![
        Event {
            t,
            kind: EventKind::Comms {
                link: Link::Up,
                cause: CommsCause::Nominal,
                one_way_delay_s: 760.0,
                expected_restore_t: None,
            },
        },
        Event {
            t,
            kind: EventKind::EarthMessage {
                sent_t: t,
                message: scenario::placeholder_goal(),
            },
        },
    ];
    for event in &events {
        log.record(
            t,
            LogEntry::Observed {
                event: event.clone(),
            },
        )?;
    }

    let decision = coordinator.decide(Observation {
        seq: 0,
        t,
        events,
        robots: scenario::placeholder_states(&scenario),
    })?;
    for command in &decision.commands {
        log.record(
            t,
            LogEntry::Commanded {
                command: command.clone(),
            },
        )?;
    }
    for message in &decision.to_earth {
        log.record(
            t,
            LogEntry::EarthDownlink {
                deliver_t: None,
                message: message.clone(),
            },
        )?;
    }

    let reason = EndReason::TimeLimit;
    let report = RunReport {
        coordinator: coordinator.info.clone(),
        decisions: 1,
        commands: decision.commands.len(),
    };
    coordinator.finish(End { t, reason })?;
    log.record(t, LogEntry::RunEnd { reason })?;
    Ok(report)
}
