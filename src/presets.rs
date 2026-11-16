//! The Standard track presets: Small, Medium and Large. Each is a crew with
//! a defined capability mix plus the outpost it works from. Every preset has
//! the same site and the same landing-pad goal, and the base station is the
//! same in every one.
//!
//! All of this belongs to a [`Catalog`]: each catalog has its own designs,
//! crews and station power, and every function here takes the catalog
//! first. What follows describes catalog 1.0 (4, 10 and 20 robots); the
//! catalog 1.1 section comes after it.
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
//!
//! **Catalog 1.1** keeps the four designs, the nesting and the outposts'
//! layout, and changes the numbers:
//!
//! - **Sourced rates.** The designs work at published rates
//!   (`docs/simulator.md` §19). A square metre takes about seven hours and
//!   37 kWh to sinter, so the pad takes weeks and sintering is nearly all of
//!   the work.
//! - **Crews of 6, 12 and 20,** half of them compactor-sinterers: (1, 1, 1,
//!   3), (2, 2, 2, 6) and (3, 4, 3, 10) scouts, excavators, haulers and
//!   compactor-sinterers.
//! - **Stations at 20 kW a slot,** so a sinterer's 100 kWh pack fills in a
//!   few hours and the slots can together draw more than the plant supplies.
//!
//! **The fleet design track** lets a coordinator bring a crew of its own
//! ([`CrewMix`]): 4 to 20 robots of the catalog's four designs, within a
//! launch-mass budget ([`mass_budget_kg`]). Every such crew works from the
//! Medium outpost, and every one is scored against the same reference
//! ([`fleet_reference_sinterers`]), so entries compete on the crew and on
//! how it is used.

use crate::scenario::{DEPOT, STATION};
use crate::sim::params::{Catalog, CrewMix, FLEET_OUTPOST, Lineup, Preset};
use arcadia_protocol::*;
use std::fmt;

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

    /// A robot of this design in `catalog`, with the given id.
    pub fn spec(self, catalog: Catalog, robot_id: &str) -> RobotSpec {
        match catalog {
            Catalog::V1_0 => self.spec_1_0(robot_id),
            Catalog::V1_1 => self.spec_1_1(robot_id),
        }
    }

    /// Catalog 1.1's design: the same four robots at sourced rates
    /// (`docs/simulator.md` §19 cites each). Speeds are those of VIPER-class
    /// rovers. Digging and loading follow RASSOR and IPEx scaled by mass;
    /// grading, compaction and sintering follow Metzger & Autry's landing-pad
    /// study. A square metre takes about seven hours and 37 kWh to sinter, so
    /// the compactor-sinterer carries a pack of two cells' worth.
    fn spec_1_1(self, robot_id: &str) -> RobotSpec {
        use CapabilityName::*;
        match self {
            Design::Scout => robot(
                robot_id,
                self,
                (90.0, 1500.0, 0.2, 0.2, 15.0),
                vec![
                    cap(Scan, 4.0, 0.05, None, Some(10.0)),
                    cap(Repair, 0.0005, 400.0, None, None),
                ],
            ),
            Design::Excavator => robot(
                robot_id,
                self,
                (420.0, 6000.0, 0.1, 0.9, 50.0),
                vec![
                    cap(Dig, 1.4e-4, 500.0, None, None),
                    cap(Grade, 0.1, 11.0, None, None),
                    cap(Haul, 0.2, 0.01, Some(250.0), None),
                ],
            ),
            Design::Hauler => robot(
                robot_id,
                self,
                (300.0, 12000.0, 0.2, 0.7, 40.0),
                vec![
                    cap(Haul, 0.2, 0.008, Some(400.0), None),
                    cap(Charge, 0.5, 0.0, Some(8000.0), None),
                    cap(Relay, 0.0, 0.0, None, Some(150.0)),
                ],
            ),
            Design::CompactorSinterer => robot(
                robot_id,
                self,
                (900.0, 100_000.0, 0.1, 1.0, 50.0),
                vec![
                    cap(Compact, 0.05, 4.0, None, None),
                    cap(Sinter, 4e-5, 37_000.0, None, None),
                ],
            ),
        }
    }

    /// Catalog 1.0's design.
    fn spec_1_0(self, robot_id: &str) -> RobotSpec {
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
fn added(catalog: Catalog, preset: Preset) -> &'static [(Design, u32)] {
    match catalog {
        Catalog::V1_0 => added_1_0(preset),
        Catalog::V1_1 => added_1_1(preset),
    }
}

/// Catalog 1.1: 6, 12 and 20 robots. Sintering is nearly all of the work, so
/// half of every crew sinters.
fn added_1_1(preset: Preset) -> &'static [(Design, u32)] {
    use Design::*;
    match preset {
        Preset::Small => &[
            (Scout, 1),
            (Excavator, 1),
            (Hauler, 1),
            (CompactorSinterer, 3),
        ],
        Preset::Medium => &[
            (Scout, 1),
            (Excavator, 1),
            (Hauler, 1),
            (CompactorSinterer, 3),
        ],
        Preset::Large => &[
            (Scout, 1),
            (Excavator, 2),
            (Hauler, 1),
            (CompactorSinterer, 4),
        ],
    }
}

fn added_1_0(preset: Preset) -> &'static [(Design, u32)] {
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
pub fn mix(catalog: Catalog, preset: Preset) -> [(Design, u32); 4] {
    Design::ALL.map(|d| {
        let n = tiers(preset)
            .flat_map(|t| added(catalog, t).iter())
            .filter(|(design, _)| *design == d)
            .map(|(_, n)| n)
            .sum();
        (d, n)
    })
}

/// The preset's crew. Ids number each design from 1 (`cp-1`, `cp-2`, ...),
/// and each preset's crew starts with the whole crew of the one below it.
pub fn crew(catalog: Catalog, preset: Preset) -> Vec<RobotSpec> {
    let mut count = [0u32; 4];
    let mut robots = Vec::new();
    for tier in tiers(preset) {
        for &(design, n) in added(catalog, tier) {
            let k = Design::ALL
                .iter()
                .position(|&d| d == design)
                .expect("a design");
            for _ in 0..n {
                count[k] += 1;
                let id = format!("{}-{}", design.prefix(), count[k]);
                robots.push(design.spec(catalog, &id));
            }
        }
    }
    robots
}

/// Where the crew starts: in rows of ten eastward from (12, 10), between
/// the first charge station and the base station.
pub fn starts(catalog: Catalog, preset: Preset) -> Vec<GridCoord> {
    starts_of(crew_size(catalog, preset))
}

/// Where a crew of `robots` starts, in crew order.
fn starts_of(robots: u32) -> Vec<GridCoord> {
    (0..robots)
        .map(|i| GridCoord {
            x: 12 + i % 10,
            y: 10 + i / 10,
        })
        .collect()
}

/// `schemas/crew.schema.json`: the JSON Schema (draft 2020-12) of a crew
/// file.
pub const CREW_SCHEMA: &str = include_str!("../schemas/crew.schema.json");

/// The fewest and the most robots a fleet design track crew may have.
pub const FLEET_ROBOTS: (u32, u32) = (4, 20);

/// The launch mass a fleet design track crew may have in all, kg: one value
/// per catalog. It is set a little under the mass of the catalog's Large
/// crew, so a crew cannot simply be the biggest one: 20 robots fit only if
/// few of them are the heavy compactor-sinterers.
pub fn mass_budget_kg(catalog: Catalog) -> f64 {
    match catalog {
        Catalog::V1_0 => 6_000.0,
        Catalog::V1_1 => 10_000.0,
    }
}

/// The mass of a crew in `catalog`, kg.
pub fn crew_mass_kg(catalog: Catalog, crew: CrewMix) -> f64 {
    Design::ALL
        .into_iter()
        .zip(crew.counts())
        .map(|(d, n)| f64::from(n) * d.spec(catalog, "").mass_kg)
        .sum()
}

/// Why a crew may not enter the fleet design track.
#[derive(Debug, Clone, PartialEq)]
pub enum CrewError {
    /// Fewer or more robots than the track allows.
    Size { robots: u32 },
    /// Heavier than the catalog's budget.
    Mass { catalog: Catalog, mass_kg: f64 },
}

impl fmt::Display for CrewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            CrewError::Size { robots } => write!(
                f,
                "a crew has {} to {} robots; this one has {robots}",
                FLEET_ROBOTS.0, FLEET_ROBOTS.1
            ),
            CrewError::Mass { catalog, mass_kg } => write!(
                f,
                "this crew weighs {mass_kg:.0} kg; catalog {catalog}'s mass budget is {:.0} kg",
                mass_budget_kg(catalog)
            ),
        }
    }
}

impl std::error::Error for CrewError {}

/// Check a crew against the fleet design track's rules: 4 to 20 robots, and
/// no more mass than the catalog's budget. A crew may leave a design out,
/// though a pad needs all four.
pub fn check_crew(catalog: Catalog, crew: CrewMix) -> Result<(), CrewError> {
    let robots = crew.total();
    if !(FLEET_ROBOTS.0..=FLEET_ROBOTS.1).contains(&robots) {
        return Err(CrewError::Size { robots });
    }
    let mass_kg = crew_mass_kg(catalog, crew);
    if mass_kg > mass_budget_kg(catalog) {
        return Err(CrewError::Mass { catalog, mass_kg });
    }
    Ok(())
}

/// A fleet design track crew: its robots design by design, with ids numbered
/// from 1 within each design, as a preset's are.
pub fn custom_crew(catalog: Catalog, crew: CrewMix) -> Vec<RobotSpec> {
    Design::ALL
        .into_iter()
        .zip(crew.counts())
        .flat_map(|(d, n)| (1..=n).map(move |k| d.spec(catalog, &format!("{}-{k}", d.prefix()))))
        .collect()
}

/// The robots of a scenario, and where each starts.
pub fn lineup(catalog: Catalog, who: Lineup) -> (Vec<RobotSpec>, Vec<GridCoord>) {
    let robots = match who {
        Lineup::Preset(preset) => crew(catalog, preset),
        Lineup::Crew(mix) => custom_crew(catalog, mix),
    };
    let starts = starts_of(robots.len() as u32);
    (robots, starts)
}

/// The sinterers the fleet design track's reference build has: as many
/// compactor-sinterers as the mass budget allows, and no more than a crew
/// may have robots. Every crew on the track is scored against the time
/// these would take to sinter the pad (`docs/scoring.md` §1), so a crew
/// with few sinterers cannot score as well as one with many.
pub fn fleet_reference_sinterers(catalog: Catalog) -> u32 {
    let each = Design::CompactorSinterer.spec(catalog, "").mass_kg;
    ((mass_budget_kg(catalog) / each).floor() as u32).min(FLEET_ROBOTS.1)
}

/// The outpost of a scenario: the preset's, or the fleet outpost for a crew
/// of the coordinator's own.
pub fn outpost(who: Lineup) -> Preset {
    match who {
        Lineup::Preset(preset) => preset,
        Lineup::Crew(_) => FLEET_OUTPOST,
    }
}

/// Number of robots in the preset's crew.
pub fn crew_size(catalog: Catalog, preset: Preset) -> u32 {
    mix(catalog, preset).iter().map(|(_, n)| n).sum()
}

/// What a charge station delivers in all, W, over its two slots.
fn station_power_w(catalog: Catalog) -> f64 {
    match catalog {
        Catalog::V1_0 => 6000.0,
        // 20 kW a slot, so a sinterer's pack fills in a few hours.
        Catalog::V1_1 => 40_000.0,
    }
}

/// Charge stations: Small's `st-1`; Medium adds its mirror image east of the
/// base station; Large adds one on each side of the pad. Each has 2 slots;
/// in catalog 1.0 it delivers 6 kW over them.
pub fn stations(catalog: Catalog, preset: Preset) -> Vec<ChargeStation> {
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
            max_power_w: station_power_w(catalog),
            slots: 2,
        })
        .collect()
}

/// Maintenance depots, placed beside the stations in the same way. They are
/// the same in every catalog.
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
    fn catalog_1_0_crews_have_the_documented_sizes_and_mixes() {
        let c = Catalog::V1_0;
        let counts = |p| mix(c, p).map(|(_, n)| n);
        assert_eq!(counts(Preset::Small), [1, 1, 1, 1]);
        assert_eq!(counts(Preset::Medium), [2, 2, 2, 4]);
        assert_eq!(counts(Preset::Large), [4, 4, 4, 8]);
        for (p, n) in [
            (Preset::Small, 4),
            (Preset::Medium, 10),
            (Preset::Large, 20),
        ] {
            assert_eq!(crew(c, p).len(), n);
            assert_eq!(crew_size(c, p), n as u32);
            assert_eq!(starts(c, p).len(), n);
            for s in stations(c, p) {
                assert_eq!(s.max_power_w / f64::from(s.slots), 3000.0);
            }
        }
        let ids: Vec<String> = crew(c, Preset::Small)
            .into_iter()
            .map(|r| r.robot_id)
            .collect();
        assert_eq!(ids, ["sc-1", "ex-1", "hl-1", "cp-1"]);
    }

    #[test]
    fn catalog_1_1_crews_have_the_documented_sizes_and_mixes() {
        let c = Catalog::V1_1;
        let counts = |p| mix(c, p).map(|(_, n)| n);
        assert_eq!(counts(Preset::Small), [1, 1, 1, 3]);
        assert_eq!(counts(Preset::Medium), [2, 2, 2, 6]);
        assert_eq!(counts(Preset::Large), [3, 4, 3, 10]);
        for (p, n) in [
            (Preset::Small, 6),
            (Preset::Medium, 12),
            (Preset::Large, 20),
        ] {
            assert_eq!(crew(c, p).len(), n);
            assert_eq!(starts(c, p).len(), n);
            for s in stations(c, p) {
                assert_eq!(s.max_power_w / f64::from(s.slots), 20_000.0);
            }
        }
        let ids: Vec<String> = crew(c, Preset::Small)
            .into_iter()
            .map(|r| r.robot_id)
            .collect();
        assert_eq!(ids, ["sc-1", "ex-1", "hl-1", "cp-1", "cp-2", "cp-3"]);
        // A sinterer's pack holds two cells' sintering with room to spare,
        // and its sinter draws about 5.3 kW.
        let cp = Design::CompactorSinterer.spec(c, "cp-1");
        let sinter = cp.capability(CapabilityName::Sinter).unwrap();
        assert!(cp.battery_capacity_wh > 2.5 * sinter.energy_wh_per_unit);
        let draw_w = sinter.rate * sinter.energy_wh_per_unit * 3600.0;
        assert!((draw_w - 5328.0).abs() < 1.0, "{draw_w}");
        // The same four designs, by model and capabilities, in both catalogs.
        for d in Design::ALL {
            let (old, new) = (d.spec(Catalog::V1_0, "x"), d.spec(c, "x"));
            assert_eq!(old.model, new.model);
            let names = |r: &RobotSpec| r.capabilities.iter().map(|c| c.name).collect::<Vec<_>>();
            assert_eq!(names(&old), names(&new));
        }
    }

    #[test]
    fn every_preset_has_every_capability_and_medium_and_large_have_two_of_each() {
        for c in Catalog::ALL {
            for p in Preset::ALL {
                let crew = crew(c, p);
                assert_eq!(crew.len(), crew_size(c, p) as usize, "{c} {p}");
                assert!(crew.len() <= 20, "{c} {p}: at most twenty robots");
                for cap in EVERY_CAPABILITY {
                    let n = crew.iter().filter(|r| r.capability(cap).is_some()).count();
                    let floor = if p == Preset::Small { 1 } else { 2 };
                    assert!(n >= floor, "{c} {p}: {cap:?} on {n} robots");
                }
            }
        }
    }

    #[test]
    fn presets_nest_and_ids_are_unique() {
        for c in Catalog::ALL {
            for (small, big) in [
                (Preset::Small, Preset::Medium),
                (Preset::Medium, Preset::Large),
            ] {
                let (a, b) = (crew(c, small), crew(c, big));
                assert!(a.len() < b.len(), "{c}: {big} adds robots to {small}");
                assert_eq!(a[..], b[..a.len()]);
                assert_eq!(starts(c, small)[..], starts(c, big)[..a.len()]);
                let (few, many) = (stations(c, small), stations(c, big));
                assert_eq!(few[..], many[..few.len()]);
                assert_eq!(depots(small)[..], depots(big)[..depots(small).len()]);
            }
            for p in Preset::ALL {
                let ids: BTreeSet<String> = crew(c, p).into_iter().map(|r| r.robot_id).collect();
                assert_eq!(ids.len(), crew_size(c, p) as usize);
                let cells: BTreeSet<(u32, u32)> = starts(c, p).iter().map(|c| (c.x, c.y)).collect();
                assert_eq!(
                    cells.len(),
                    crew_size(c, p) as usize,
                    "one robot per start cell"
                );
            }
            assert_eq!(stations(c, Preset::Large).len(), 4);
        }
        assert_eq!(depots(Preset::Medium).len(), 2);
    }

    #[test]
    fn the_outpost_stands_in_coverage_off_the_pad() {
        use crate::sim::params::{Params, Variant};
        for c in Catalog::ALL {
            for p in Preset::ALL {
                let range = Params::scenario(c, p, Variant::Nominal).base_station_range_m;
                let places = stations(c, p)
                    .into_iter()
                    .map(|s| s.position)
                    .chain(depots(p).into_iter().map(|d| d.position))
                    .chain(starts(c, p));
                for at in places {
                    assert!(at.x < SITE_SIZE && at.y < SITE_SIZE, "{c} {p}: {at:?}");
                    assert!(within(at, BASE_STATION, range, 1.0), "{c} {p}: {at:?}");
                    let on_pad = (PAD.min.x..=PAD.max.x).contains(&at.x)
                        && (PAD.min.y..=PAD.max.y).contains(&at.y);
                    assert!(!on_pad, "{c} {p}: {at:?}");
                }
                for s in stations(c, p) {
                    assert!(s.slots > 0 && s.max_power_w > 0.0, "{c} {p}");
                }
            }
        }
    }

    #[test]
    fn the_fleet_track_admits_crews_by_size_and_mass() {
        let mix = |sc, ex, hl, cp| CrewMix {
            scout: sc,
            excavator: ex,
            hauler: hl,
            compactor_sinterer: cp,
        };
        for c in Catalog::ALL {
            // Small and Medium fit; the Large crew is over the budget.
            let of = |p| {
                let [sc, ex, hl, cp] = super::mix(c, p).map(|(_, n)| n);
                mix(sc, ex, hl, cp)
            };
            assert_eq!(check_crew(c, of(Preset::Small)), Ok(()));
            assert_eq!(check_crew(c, of(Preset::Medium)), Ok(()));
            let large = of(Preset::Large);
            let over = check_crew(c, large).unwrap_err();
            assert_eq!(
                over,
                CrewError::Mass {
                    catalog: c,
                    mass_kg: crew_mass_kg(c, large)
                }
            );
            assert!(over.to_string().contains("mass budget"), "{over}");
            // A preset's crew as a custom crew is the same robots, listed
            // design by design.
            for p in [Preset::Small, Preset::Medium] {
                let (mut robots, starts) = lineup(c, Lineup::Crew(of(p)));
                let mut preset = crew(c, p);
                assert_eq!(starts, super::starts(c, p));
                assert_eq!(lineup(c, Lineup::Preset(p)), (preset.clone(), starts));
                robots.sort_by(|a, b| a.robot_id.cmp(&b.robot_id));
                preset.sort_by(|a, b| a.robot_id.cmp(&b.robot_id));
                assert_eq!(robots, preset);
            }
            // Too few and too many robots, whatever they weigh.
            for (crew, robots) in [(mix(1, 1, 1, 0), 3), (mix(21, 0, 0, 0), 21)] {
                let e = check_crew(c, crew).unwrap_err();
                assert_eq!(e, CrewError::Size { robots });
                assert!(e.to_string().contains("4 to 20 robots"), "{e}");
            }
            // Twenty scouts fit in every budget.
            assert_eq!(check_crew(c, mix(20, 0, 0, 0)), Ok(()));
            // The reference crew is all sinterers, as many as the budget
            // carries, and itself fits.
            let n = fleet_reference_sinterers(c);
            assert_eq!(check_crew(c, mix(0, 0, 0, n.max(4))), Ok(()));
            assert!(n == FLEET_ROBOTS.1 || check_crew(c, mix(0, 0, 0, n + 1)).is_err());
        }
        assert_eq!(crew_mass_kg(Catalog::V1_1, mix(1, 1, 1, 3)), 3_510.0);
        assert_eq!(crew_mass_kg(Catalog::V1_1, mix(3, 4, 3, 10)), 11_850.0);
        assert_eq!(crew_mass_kg(Catalog::V1_0, mix(4, 4, 4, 8)), 7_240.0);
        assert_eq!(fleet_reference_sinterers(Catalog::V1_1), 11);
        assert_eq!(fleet_reference_sinterers(Catalog::V1_0), 12);
        assert_eq!(outpost(Lineup::Crew(mix(1, 1, 1, 1))), Preset::Medium);
        assert_eq!(outpost(Lineup::Preset(Preset::Large)), Preset::Large);
        let ids: Vec<String> = custom_crew(Catalog::V1_1, mix(2, 0, 1, 2))
            .into_iter()
            .map(|r| r.robot_id)
            .collect();
        assert_eq!(ids, ["sc-1", "sc-2", "hl-1", "cp-1", "cp-2"]);
    }
}
