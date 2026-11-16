//! Energy (semantics §7): power modes, the grid balance and the energy metrics.

mod support;

use arcadia_protocol::*;
use arcadiabench::sim::params::{Catalog, Params, Preset, Variant};
use support::*;

const SOL_S: f64 = 88_775.0;

fn power_events(records: &[LogRecord]) -> Vec<(f64, f64, f64)> {
    records
        .iter()
        .filter_map(|r| match &r.entry {
            LogEntry::Observed { event } => match event.kind {
                EventKind::Power {
                    fission_w,
                    storage_wh,
                    ..
                } => Some((event.t, fission_w, storage_wh)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[test]
fn the_scenario_reports_each_power_mode() {
    for v in Variant::ALL {
        let p = Params::variant(v);
        let (_, records, _) = run(Idle, 1, &p);
        let LogEntry::RunStart { scenario, .. } = &records[0].entry else {
            panic!("no run_start");
        };
        assert_eq!(scenario.power.mode, p.power_mode, "{v}");
        assert_eq!(scenario.power.fission_w, p.fission_w, "{v}");
        assert_eq!(scenario.power.solar_peak_w, p.solar_peak_w, "{v}");
        assert_eq!(
            scenario.power.storage_capacity_wh, p.bank_capacity_wh,
            "{v}"
        );
        assert_eq!(scenario.power.essential_load_w, p.essential_load_w, "{v}");
        assert_eq!(scenario.base_station.range_m, p.base_station_range_m, "{v}");
        assert!(
            power_events(&records)
                .iter()
                .all(|&(_, f, _)| f == p.fission_w)
        );
    }
    assert_eq!(Params::solar_only().power_mode, PowerMode::SolarOnly);
    assert_eq!(Params::nuclear_heavy().power_mode, PowerMode::NuclearHeavy);
}

#[test]
fn an_empty_bank_without_fission_runs_a_deficit_until_the_sun_is_up() {
    // Solar only, starting empty at midnight: the essential load goes short
    // until well after sunrise, then the day fills the bank.
    let p = Params {
        bank_initial_frac: 0.0,
        ..Params::solar_only()
    };
    let (report, records, _) = run(Idle, 1, &p);
    let m = &report.metrics.energy;
    let sunrise = 0.25 * SOL_S;
    assert!(
        m.deficit_s > sunrise && m.deficit_s < sunrise + 2_000.0,
        "{}",
        m.deficit_s
    );
    assert_eq!(m.min_storage_wh, 0.0);
    assert_eq!(m.min_storage_fraction, 0.0);
    // Storage recovers during the day.
    assert!(
        power_events(&records)
            .iter()
            .any(|&(t, _, s)| t < SOL_S && s > 50_000.0)
    );

    // With fission covering the essential load there is never a deficit.
    let (nominal, _, _) = run(Idle, 1, &Params::default());
    assert_eq!(nominal.metrics.energy.deficit_s, 0.0);
    let initial = Params::default().bank_capacity_wh * Params::default().bank_initial_frac;
    assert!(nominal.metrics.energy.min_storage_wh <= initial);
    assert!(nominal.metrics.energy.min_storage_wh > 0.0);
}

#[test]
fn site_truth_agrees_with_power_events_and_charging_is_counted() {
    let mut n = 0;
    let (report, records, _) = run(
        Script::new(|obs: &Observation, d: &mut Decision| {
            // Keep the hauler charging at the station.
            let hl = robot(obs, "hl-1");
            if hl.current_command.is_none() && hl.status == RobotStatus::Idle && n < 3 {
                n += 1;
                d.commands.push(cmd(
                    &format!("w{n}"),
                    "hl-1",
                    Action::Wait {
                        duration_s: 20_000.0,
                    },
                ));
                d.commands.push(cmd(
                    &format!("c{n}"),
                    "ex-1",
                    Action::Charge {
                        source: "st-1".into(),
                        until_wh: None,
                    },
                ));
            }
        }),
        2,
        &Params::default(),
    );
    // At every power event the truth record of the same observation has the same storage.
    for (t, _, storage) in power_events(&records) {
        let site = records
            .iter()
            .find_map(|r| match &r.entry {
                LogEntry::Truth { site, .. } if r.t == t => Some(site.clone()),
                _ => None,
            })
            .expect("a truth record at every decision point");
        assert!((site.storage_wh - storage).abs() < 1e-6, "t = {t}");
        // The link in truth is the one the latest `comms` event reported.
        let reported = records
            .iter()
            .filter_map(|r| match &r.entry {
                LogEntry::Observed { event } if event.t <= t => match event.kind {
                    EventKind::Comms { link, .. } => Some(link),
                    _ => None,
                },
                _ => None,
            })
            .next_back()
            .expect("a comms event at the start");
        assert_eq!(site.link, reported, "t = {t}");
    }
    let m = &report.metrics.energy;
    assert!(m.grid_energy_wh > 0.0, "the station delivered energy");
    // Every robot ends the run empty, so all it drew is its capacity plus what it received.
    let capacity = 1500.0 + 6000.0 + 12000.0 + 10000.0;
    assert!(m.robot_energy_wh >= capacity, "{}", m.robot_energy_wh);
}

#[test]
fn sintering_a_cell_in_catalog_1_1_takes_hours_and_a_third_of_a_pack() {
    // Three cells beside the outpost are prepared and sintered one at a
    // time: the first in full, the second cancelled early and then redone,
    // and the third with too little left in the pack.
    let p = Params {
        faults_enabled: false,
        ..Params::scenario(Catalog::V1_1, Preset::Small, Variant::Nominal)
    };
    let cells = [15, 16, 17].map(|x| GridCoord { x, y: 12 });
    let one = |c: GridCoord| GridRect { min: c, max: c };
    let mut level: Vec<f64> = Vec::new();
    let (mut step, mut busy, mut cancel_at) = (0usize, false, None::<f64>);
    let script = Script::new(|obs: &Observation, d: &mut Decision| {
        for e in &obs.events {
            match &e.kind {
                // Cut each cell a little below where it was scanned.
                EventKind::ScanResult { cells: seen, .. } => {
                    level = cells
                        .iter()
                        .map(|c| {
                            let o = seen.iter().find(|o| o.at == *c).expect("scanned");
                            o.cell.elevation_m.expect("an elevation") - 0.06
                        })
                        .collect();
                }
                EventKind::CommandCompleted { command_id, .. }
                | EventKind::CommandFailed { command_id, .. }
                | EventKind::CommandRejected { command_id, .. }
                    if command_id.starts_with('s') =>
                {
                    busy = false;
                }
                _ => {}
            }
        }
        if let Some(t) = cancel_at {
            if obs.t >= t {
                cancel_at = None;
                let target_command = "s11".into();
                d.commands
                    .push(cmd("cancel", "cp-1", Action::Cancel { target_command }));
            } else {
                d.wake_at = Some(t);
            }
            return;
        }
        if busy {
            return;
        }
        let k = step.saturating_sub(1) % 3;
        let (robot, action) = match step {
            0 => (
                "sc-1",
                Action::Scan {
                    area: GridRect {
                        min: cells[0],
                        max: cells[2],
                    },
                },
            ),
            1..=3 => (
                "ex-1",
                Action::Dig {
                    area: one(cells[k]),
                    target_elevation_m: level[k],
                    spoil_to: Some(GridCoord { x: 16, y: 14 }),
                },
            ),
            4..=6 => (
                "ex-1",
                Action::Grade {
                    area: one(cells[k]),
                    target_elevation_m: level[k],
                },
            ),
            7..=9 => (
                "cp-1",
                Action::Compact {
                    area: one(cells[k]),
                },
            ),
            10 => (
                "cp-1",
                Action::Sinter {
                    area: one(cells[0]),
                },
            ),
            11 | 12 => (
                "cp-1",
                Action::Sinter {
                    area: one(cells[1]),
                },
            ),
            13 => (
                "cp-1",
                Action::Sinter {
                    area: one(cells[2]),
                },
            ),
            _ => return,
        };
        d.commands.push(cmd(&format!("s{step}"), robot, action));
        if step == 11 {
            cancel_at = Some(obs.t + 2_500.0);
            d.wake_at = cancel_at;
        }
        busy = true;
        step += 1;
    });
    let (_, records, agent) = run(script, 2, &p);
    let events: Vec<&Event> = events(&agent.observations).collect();
    let rejected: Vec<&&Event> = events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::CommandRejected { .. }))
        .collect();
    assert!(rejected.is_empty(), "{rejected:?}");
    let ended = |id: &str| {
        events
            .iter()
            .find_map(|e| match &e.kind {
                EventKind::CommandCompleted {
                    command_id,
                    energy_used_wh,
                    ..
                } if command_id == id => Some((e.t, None, *energy_used_wh)),
                EventKind::CommandFailed {
                    command_id,
                    reason,
                    energy_used_wh,
                    ..
                } if command_id == id => Some((e.t, Some(*reason), *energy_used_wh)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{id} never ended"))
    };
    let accepted = |id: &str| {
        events
            .iter()
            .find_map(|e| match &e.kind {
                EventKind::CommandAccepted { command_id, .. } if command_id == id => Some(e.t),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{id} was not accepted"))
    };
    let battery = |t: f64| {
        let cp = truth_at(&records, t);
        let cp = cp.iter().find(|r| r.robot_id == "cp-1").expect("cp-1");
        (cp.battery_wh, cp.status)
    };

    // One square metre: 37 kWh for the sinter and about seven hours, with
    // the robot's own draw on top. The pack starts at 100 kWh.
    let (done, reason, used) = ended("s10");
    assert_eq!(reason, None);
    let hours = (done - accepted("s10")) / 3600.0;
    assert!((3.0..16.0).contains(&hours), "{hours} h");
    let own = hours * 50.0;
    assert!(
        used > 37_000.0 && used < 37_000.0 + 1.5 * own + 50.0,
        "{used}"
    );
    assert!((battery(done).0 - (100_000.0 - used)).abs() < 600.0);

    // Cancelled 2,500 s into a step of hours: the unused part of what the
    // step drew comes back, and the cell is not sintered.
    let (stopped, reason, part) = ended("s11");
    assert_eq!(reason, Some(FailReason::Cancelled));
    assert!(part > 500.0 && part < 0.5 * 37_000.0, "{part}");
    assert!((stopped - accepted("s11") - 2_500.0).abs() < 600.0);

    // Done again in full, then a third cell with a quarter of a pack left:
    // the step cannot be paid for, and the robot goes offline with an empty
    // battery (semantics: a shortfall drains the pack).
    let (again, reason, used_again) = ended("s12");
    assert_eq!(reason, None);
    assert!(used_again > 37_000.0, "{used_again}");
    let (left, _) = battery(again);
    assert!(left > 15_000.0 && left < 30_000.0, "{left}");
    let (failed, reason, _) = ended("s13");
    assert_eq!(reason, Some(FailReason::InsufficientEnergy));
    let (empty, status) = battery(failed + 1.0);
    assert_eq!((empty, status), (0.0, RobotStatus::Offline));
}
