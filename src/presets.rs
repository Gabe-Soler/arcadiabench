//! The Standard track presets: Small (4 robots), Medium (10) and Large (20).
//! Each is a crew with a defined capability mix plus the outpost it works
//! from. Every preset has the same site and the same landing-pad goal, and
//! the base station is the same in every one.
//!
//! - **Four designs.** Every robot is one of the Small crew's four designs,
//!   with unchanged specs. A rate, a battery or a speed therefore means the
//!   same thing in every preset, and the results differ only by crew size
//!   and coordination.
//! - **Mix.** Sintering is the slowest step, and it accounts for most of the
//!   work: when ArcadiaOS builds the pad on Small, its compactor-sinterer is
//!   busy (sintering, compacting and charging for both) about 20 of the
//!   crew's 33 busy robot-hours. So beyond the minimal Small crew (one robot
//!   of each design), compactor-sinterers make up 40% of the crew. Every other
//!   design is doubled in Medium and doubled again in Large. In Medium and
//!   Large every capability is carried by at least two robots, so no single
//!   breakdown strands a capability.
//! - **Nesting.** Medium is the Small crew plus six robots, and Large is the
//!   Medium crew plus ten, with the same ids. A coordinator that only knows
//!   `sc-1`, `ex-1`, `hl-1` and `cp-1` still runs on every preset.
//! - **Outpost.** Charge slots and depots scale with the crew: Medium
//!   mirrors the Small outpost's station and depot to the east of the base
//!   station, and Large adds a pair on each side of the pad. Every station
//!   keeps the Small station's 3 kW per slot. Everything stands inside the
//!   base station's coverage. The power plant is the variant's in every
//!   preset, so a bigger crew must share the same supply.

use crate::scenario::{DEPOT, STATION};
use crate::sim::params::Preset;
use arcadia_protocol::*;

/// A robot design: what a robot can do, how fast, and at what energy cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Design {
    /// Light and quick: surveys and verifies the pad, and repairs robots in the field.
    Scout,
    /// Cuts, levels and carries regolith.
    Excavator,
    /// Moves regolith in bulk, recharges robots from its tank, and relays comms.
    Hauler,
    /// Densifies the graded surface, then sinters it.
    CompactorSinterer,
}

impl Design {
    pub const ALL: [Design; 4] = [
        Design::Scout,
        Design::Excavator,
        Design::Hauler,
        Design::CompactorSinterer,
    ];

    /// The robot id prefix: `sc`, `ex`, `hl` or `cp`.
    pub fn prefix(self) -> &'static str {
        match self {
            Design::Scout => "sc",
            Design::Excavator => "ex",
            Design::Hauler => "hl",
            Design::CompactorSinterer => "cp",
        }
    }

    /// The model name a robot of this design reports.
    pub fn model(self) -> &'static str {
        match self {
            Design::Scout => "scout",
            Design::Excavator => "excavator",
            Design::Hauler => "hauler",
            Design::CompactorSinterer => "compactor-sinterer",
        }
    }

    /// A robot of this design with the given id.
    pub fn spec(self, robot_id: &str) -> RobotSpec {
        use CapabilityName::*;
        match self {
            Design::Scout => robot(
                robot_id,
                self,
                (90.0, 1500.0, 0.5, 0.2, 8.0),
                vec![
                    cap(Scan, 4.0, 0.05, None, Some(10.0)),
                    cap(Repair, 0.0005, 400.0, None, None),
                ],
            ),
            Design::Excavator => robot(
                robot_id,
                self,
                (420.0, 6000.0, 0.15, 0.9, 35.0),
                vec![
                    cap(Dig, 0.004, 100.0, None, None),
                    cap(Grade, 0.05, 8.0, None, None),
                    cap(Haul, 2.0, 0.01, Some(250.0), None),
                ],
            ),
            Design::Hauler => robot(
                robot_id,
                self,
                (300.0, 12000.0, 0.3, 0.7, 20.0),
                vec![
                    cap(Haul, 3.0, 0.008, Some(400.0), None),
                    cap(Charge, 0.5, 0.0, Some(8000.0), None),
                    cap(Relay, 0.0, 0.0, None, Some(150.0)),
                ],
            ),
            Design::CompactorSinterer => robot(
                robot_id,
                self,
                (500.0, 10000.0, 0.15, 1.0, 30.0),
                vec![
                    cap(Compact, 0.1, 4.0, None, None),
                    cap(Sinter, 0.01, 60.0, None, None),
                ],
            ),
        }
    }
}

/// The robots each preset adds to the one before it, in crew order.
fn added(preset: Preset) -> &'static [(Design, u32)] {
    use Design::*;
    match preset {
        Preset::Small => &[
            (Scout, 1),
            (Excavator, 1),
            (Hauler, 1),
            (CompactorSinterer, 1),
        ],
        Preset::Medium => &[
            (Scout, 1),
            (Excavator, 1),
            (Hauler, 1),
            (CompactorSinterer, 3),
        ],
        Preset::Large => &[
            (Scout, 2),
            (Excavator, 2),
            (Hauler, 2),
            (CompactorSinterer, 4),
        ],
    }
}

/// The presets up to and including `preset`, smallest first.
fn tiers(preset: Preset) -> impl Iterator<Item = Preset> {
    Preset::ALL.into_iter().take_while(move |&p| p <= preset)
}

/// How many robots of each design the preset's crew has.
pub fn mix(preset: Preset) -> [(Design, u32); 4] {
    Design::ALL.map(|d| {
        let n = tiers(preset)
            .flat_map(|t| added(t).iter())
            .filter(|(design, _)| *design == d)
            .map(|(_, n)| n)
            .sum();
        (d, n)
    })
}

/// The preset's crew. Ids number each design from 1 (`cp-1`, `cp-2`, ...),
/// and each preset's crew starts with the whole crew of the one below it.
pub fn crew(preset: Preset) -> Vec<RobotSpec> {
    let mut count = [0u32; 4];
    let mut robots = Vec::new();
    for tier in tiers(preset) {
        for &(design, n) in added(tier) {
            let k = Design::ALL
                .iter()
                .position(|&d| d == design)
                .expect("a design");
            for _ in 0..n {
                count[k] += 1;
                robots.push(design.spec(&format!("{}-{}", design.prefix(), count[k])));
            }
        }
    }
    robots
}

/// Where the crew starts: in rows of ten eastward from (12, 10), between
/// the first charge station and the base station.
pub fn starts(preset: Preset) -> Vec<GridCoord> {
    (0..crew_size(preset))
        .map(|i| GridCoord {
            x: 12 + i % 10,
            y: 10 + i / 10,
        })
        .collect()
}

/// Number of robots in the preset's crew.
pub fn crew_size(preset: Preset) -> u32 {
    mix(preset).iter().map(|(_, n)| n).sum()
}

/// Charge stations: Small's `st-1`; Medium adds its mirror image east of the
/// base station; Large adds one on each side of the pad. Each delivers 6 kW
/// over 2 slots.
pub fn stations(preset: Preset) -> Vec<ChargeStation> {
    let positions = [
        (Preset::Small, STATION),
        (Preset::Medium, GridCoord { x: 49, y: 10 }),
        (Preset::Large, GridCoord { x: 10, y: 30 }),
        (Preset::Large, GridCoord { x: 49, y: 30 }),
    ];
    positions
        .into_iter()
        .filter(|&(tier, _)| tier <= preset)
        .enumerate()
        .map(|(i, (_, position))| ChargeStation {
            station_id: format!("st-{}", i + 1),
            position,
            max_power_w: 6000.0,
            slots: 2,
        })
        .collect()
}

/// Maintenance depots, placed beside the stations in the same way.
pub fn depots(preset: Preset) -> Vec<Depot> {
    let positions = [
        (Preset::Small, DEPOT),
        (Preset::Medium, GridCoord { x: 51, y: 14 }),
        (Preset::Large, GridCoord { x: 8, y: 26 }),
        (Preset::Large, GridCoord { x: 51, y: 26 }),
    ];
    positions
        .into_iter()
        .filter(|&(tier, _)| tier <= preset)
        .enumerate()
        .map(|(i, (_, position))| Depot {
            depot_id: format!("dp-{}", i + 1),
            position,
        })
        .collect()
}

/// A robot: (mass kg, battery Wh, top speed m/s, drive Wh/m, idle W) and its capabilities.
fn robot(
    robot_id: &str,
    design: Design,
    (mass_kg, battery_wh, speed, drive_wh_per_m, idle_w): (f64, f64, f64, f64, f64),
    capabilities: Vec<CapabilitySpec>,
) -> RobotSpec {
    RobotSpec {
        robot_id: robot_id.into(),
        model: Some(design.model().into()),
        mass_kg,
        battery_capacity_wh: battery_wh,
        max_speed_m_s: speed,
        drive_energy_wh_per_m: drive_wh_per_m,
        idle_power_w: idle_w,
        capabilities,
    }
}

fn cap(
    name: CapabilityName,
    rate: f64,
    energy_wh_per_unit: f64,
    capacity: Option<f64>,
    range_m: Option<f64>,
) -> CapabilitySpec {
    CapabilitySpec {
        name,
        rate,
        energy_wh_per_unit,
        capacity,
        range_m,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{BASE_STATION, PAD, SITE_SIZE};
    use crate::sim::coverage::within;
    use std::collections::BTreeSet;

    const EVERY_CAPABILITY: [CapabilityName; 9] = {
        use CapabilityName::*;
        [
            Scan, Dig, Grade, Haul, Compact, Sinter, Charge, Relay, Repair,
        ]
    };

    #[test]
    fn crews_have_the_documented_sizes_and_mixes() {
        let counts = |p| mix(p).map(|(_, n)| n);
        assert_eq!(counts(Preset::Small), [1, 1, 1, 1]);
        assert_eq!(counts(Preset::Medium), [2, 2, 2, 4]);
        assert_eq!(counts(Preset::Large), [4, 4, 4, 8]);
        for (p, n) in [
            (Preset::Small, 4),
            (Preset::Medium, 10),
            (Preset::Large, 20),
        ] {
            assert_eq!(crew(p).len(), n);
            assert_eq!(crew_size(p), n as u32);
            assert_eq!(starts(p).len(), n);
        }
        let ids: Vec<String> = crew(Preset::Small)
            .into_iter()
            .map(|r| r.robot_id)
            .collect();
        assert_eq!(ids, ["sc-1", "ex-1", "hl-1", "cp-1"]);
    }

    #[test]
    fn every_preset_has_every_capability_and_medium_and_large_have_two_of_each() {
        for p in Preset::ALL {
            let crew = crew(p);
            for c in EVERY_CAPABILITY {
                let n = crew.iter().filter(|r| r.capability(c).is_some()).count();
                let floor = if p == Preset::Small { 1 } else { 2 };
                assert!(n >= floor, "{p}: {c:?} on {n} robots");
            }
        }
    }

    #[test]
    fn presets_nest_and_ids_are_unique() {
        for (small, big) in [
            (Preset::Small, Preset::Medium),
            (Preset::Medium, Preset::Large),
        ] {
            let (a, b) = (crew(small), crew(big));
            assert_eq!(a[..], b[..a.len()]);
            assert_eq!(starts(small)[..], starts(big)[..a.len()]);
            assert_eq!(stations(small)[..], stations(big)[..stations(small).len()]);
            assert_eq!(depots(small)[..], depots(big)[..depots(small).len()]);
        }
        for p in Preset::ALL {
            let ids: BTreeSet<String> = crew(p).into_iter().map(|r| r.robot_id).collect();
            assert_eq!(ids.len(), crew_size(p) as usize);
            let cells: BTreeSet<(u32, u32)> = starts(p).iter().map(|c| (c.x, c.y)).collect();
            assert_eq!(
                cells.len(),
                crew_size(p) as usize,
                "one robot per start cell"
            );
        }
        assert_eq!(stations(Preset::Large).len(), 4);
        assert_eq!(depots(Preset::Medium).len(), 2);
    }

    #[test]
    fn the_outpost_stands_in_coverage_off_the_pad() {
        let range = crate::sim::params::Params::default().base_station_range_m;
        for p in Preset::ALL {
            let places = stations(p)
                .into_iter()
                .map(|s| s.position)
                .chain(depots(p).into_iter().map(|d| d.position))
                .chain(starts(p));
            for c in places {
                assert!(c.x < SITE_SIZE && c.y < SITE_SIZE, "{p}: {c:?}");
                assert!(within(c, BASE_STATION, range, 1.0), "{p}: {c:?}");
                let on_pad = (PAD.min.x..=PAD.max.x).contains(&c.x)
                    && (PAD.min.y..=PAD.max.y).contains(&c.y);
                assert!(!on_pad, "{p}: {c:?}");
            }
            for s in stations(p) {
                assert_eq!(s.max_power_w / f64::from(s.slots), 3000.0);
            }
        }
    }
}
