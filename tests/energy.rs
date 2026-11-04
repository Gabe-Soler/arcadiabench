//! Energy (semantics §7): power modes, the grid balance and the energy metrics.

mod support;

use arcadia_protocol::*;
use arcadiabench::sim::params::{Params, Variant};
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
        assert_eq!(site.link, Link::Up);
    }
    let m = &report.metrics.energy;
    assert!(m.grid_energy_wh > 0.0, "the station delivered energy");
    // Every robot ends the run empty, so all it drew is its capacity plus what it received.
    let capacity = 1500.0 + 6000.0 + 12000.0 + 10000.0;
    assert!(m.robot_energy_wh >= capacity, "{}", m.robot_energy_wh);
}
