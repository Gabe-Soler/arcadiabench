//! What a baseline knows: the crew and its latest snapshots, the outpost,
//! the site as scanned, and the authority policy in effect. Everything
//! comes from `hello` and observations.

use arcadia_protocol::*;
use std::collections::{BTreeMap, BTreeSet};

/// A cell's key, `(x, y)`.
pub type Key = (u32, u32);

/// The key of a cell.
pub fn key(c: GridCoord) -> Key {
    (c.x, c.y)
}

/// The cell with a key.
pub fn cell(k: Key) -> GridCoord {
    GridCoord { x: k.0, y: k.1 }
}

/// Euclidean distance between two cell centres, in cells.
pub fn apart(a: GridCoord, b: GridCoord) -> f64 {
    let dx = f64::from(a.x) - f64::from(b.x);
    let dy = f64::from(a.y) - f64::from(b.y);
    (dx * dx + dy * dy).sqrt()
}

/// The cell of `area` nearest `c`: where a scan of `area` is entered from `c`.
pub fn entry(c: GridCoord, area: &GridRect) -> GridCoord {
    GridCoord {
        x: c.x.clamp(area.min.x, area.max.x),
        y: c.y.clamp(area.min.y, area.max.y),
    }
}

/// The cells of `area`, row by row.
pub fn cells_of(area: &GridRect) -> impl Iterator<Item = GridCoord> + '_ {
    (area.min.y..=area.max.y)
        .flat_map(move |y| (area.min.x..=area.max.x).map(move |x| GridCoord { x, y }))
}

/// The smallest rectangle holding every cell in `cells`.
pub fn bounds(cells: impl IntoIterator<Item = GridCoord>) -> Option<GridRect> {
    let mut it = cells.into_iter();
    let first = it.next()?;
    let mut r = GridRect {
        min: first,
        max: first,
    };
    for c in it {
        r.min.x = r.min.x.min(c.x);
        r.min.y = r.min.y.min(c.y);
        r.max.x = r.max.x.max(c.x);
        r.max.y = r.max.y.max(c.y);
    }
    Some(r)
}

/// The shortest a scan or a per-cell work step takes, s. The protocol
/// leaves it to the simulator; this is ArcadiaBench's documented value.
pub const MIN_WORK_S: f64 = 5.0;

/// Density assumed for boulders, kg/m³ (scans report diameters, not masses).
pub const ROCK_KG_M3: f64 = 2800.0;

/// What the last scan of a cell reported.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub elevation_m: f64,
    pub density_kg_m3: f64,
    pub depth_m: f64,
    pub surface: Surface,
    /// Volume of the boulders the scan resolved, m³.
    pub boulders_m3: f64,
}

/// A robot: its spec and what the coordinator knows of its state.
#[derive(Debug, Clone)]
pub struct Robot {
    pub spec: RobotSpec,
    /// The latest snapshot; `None` before the first observation.
    pub state: Option<RobotState>,
    /// When the robot last became free, for first-come-first-served.
    pub free_since: f64,
    /// Whether it was free at the previous decision.
    pub was_free: bool,
    /// No commands before this time, after a rejection.
    pub backoff_until: f64,
    /// Capabilities the simulator refused (`missing_capability`).
    pub lacks: BTreeSet<CapabilityName>,
}

/// Everything the baseline knows about the crew, the outpost and the site.
#[derive(Debug, Clone)]
pub struct World {
    /// The current observation's time.
    pub t: f64,
    pub seed: u64,
    pub robots: Vec<Robot>,
    pub ids: BTreeMap<String, usize>,
    pub stations: Vec<ChargeStation>,
    pub depots: Vec<Depot>,
    pub base: BaseStation,
    pub cell_m: f64,
    pub width: u32,
    pub height: u32,
    /// Orbital prior elevations.
    pub prior: BTreeMap<Key, f64>,
    /// The latest scan of each scanned cell.
    pub seen: BTreeMap<Key, Seen>,
    /// The authority policy in effect.
    pub policy: Policy,
}

impl World {
    pub fn new(hello: &Hello) -> Self {
        let s = &hello.scenario;
        Self {
            t: s.start_t,
            seed: hello.seed,
            robots: s
                .robots
                .iter()
                .map(|spec| Robot {
                    spec: spec.clone(),
                    state: None,
                    free_since: s.start_t,
                    was_free: false,
                    backoff_until: s.start_t,
                    lacks: BTreeSet::new(),
                })
                .collect(),
            ids: s
                .robots
                .iter()
                .enumerate()
                .map(|(i, r)| (r.robot_id.clone(), i))
                .collect(),
            stations: s.stations.clone(),
            depots: s.depots.clone(),
            base: s.base_station.clone(),
            cell_m: s.site.cell_size_m,
            width: s.site.width,
            height: s.site.height,
            prior: s
                .site
                .cells
                .iter()
                .filter_map(|c| c.cell.elevation_m.map(|e| (key(c.at), e)))
                .collect(),
            seen: BTreeMap::new(),
            policy: s.policy.clone(),
        }
    }

    /// Take in an observation's snapshots.
    pub fn observe(&mut self, obs: &Observation) {
        self.t = obs.t;
        for s in &obs.robots {
            if let Some(&i) = self.ids.get(&s.robot_id) {
                self.robots[i].state = Some(s.clone());
            }
        }
    }

    /// Take in a scan's cells.
    pub fn scanned(&mut self, cells: &[CellObservation]) {
        for c in cells {
            let Some(elevation_m) = c.cell.elevation_m else {
                continue;
            };
            let regolith = c.cell.regolith.as_ref();
            let boulders_m3 = c.cell.boulders.as_ref().map_or(0.0, |bs| {
                bs.iter()
                    .map(|b| std::f64::consts::PI / 6.0 * b.diameter_m.powi(3))
                    .sum()
            });
            self.seen.insert(
                key(c.at),
                Seen {
                    elevation_m,
                    density_kg_m3: regolith
                        .and_then(|r| r.bulk_density_kg_m3)
                        .unwrap_or(1550.0),
                    depth_m: regolith.and_then(|r| r.depth_m).unwrap_or(0.0),
                    surface: c.cell.surface.unwrap_or(Surface::Natural),
                    boulders_m3,
                },
            );
        }
    }

    pub fn state(&self, r: usize) -> Option<&RobotState> {
        self.robots[r].state.as_ref()
    }

    pub fn status(&self, r: usize) -> Option<RobotStatus> {
        self.state(r).map(|s| s.status)
    }

    pub fn pos(&self, r: usize) -> GridCoord {
        self.state(r)
            .map_or(GridCoord { x: 0, y: 0 }, |s| s.position)
    }

    pub fn battery(&self, r: usize) -> f64 {
        self.state(r).map_or(0.0, |s| s.battery_wh)
    }

    pub fn payload(&self, r: usize) -> f64 {
        self.state(r).map_or(0.0, |s| s.payload_kg)
    }

    pub fn capacity_wh(&self, r: usize) -> f64 {
        self.robots[r].spec.battery_capacity_wh
    }

    /// Whether the robot reports in: its snapshot is fresh.
    pub fn in_contact(&self, r: usize) -> bool {
        self.state(r).is_some_and(|s| s.in_relay_coverage)
    }

    /// Whether a robot can take a command: idle, no command running, in
    /// contact, and not backing off. Engagement by other robots' commands
    /// is tracked by the caller.
    pub fn idle(&self, r: usize) -> bool {
        self.state(r).is_some_and(|s| {
            s.status == RobotStatus::Idle && s.current_command.is_none() && s.in_relay_coverage
        }) && self.t >= self.robots[r].backoff_until
    }

    /// A capability the robot can use: it has it with a positive rate (and
    /// capacity, for hauling and charging), and the simulator never refused it.
    pub fn cap(&self, r: usize, name: CapabilityName) -> Option<&CapabilitySpec> {
        let robot = &self.robots[r];
        if robot.lacks.contains(&name) {
            return None;
        }
        robot.spec.capability(name).filter(|c| {
            let sized = !matches!(name, CapabilityName::Haul | CapabilityName::Charge)
                || c.capacity.is_some_and(|k| k > 0.0);
            sized && (c.rate > 0.0 || name == CapabilityName::Relay)
        })
    }

    pub fn can(&self, r: usize, name: CapabilityName) -> bool {
        self.cap(r, name).is_some()
    }

    /// Payload room left for dig spoil, kg (0 without `haul`).
    pub fn room_kg(&self, r: usize) -> f64 {
        let cap = self
            .cap(r, CapabilityName::Haul)
            .and_then(|c| c.capacity)
            .unwrap_or(0.0);
        (cap - self.payload(r)).max(0.0)
    }

    pub fn on_site(&self, c: GridCoord) -> bool {
        c.x < self.width && c.y < self.height
    }

    /// Within the base station's direct coverage (baselines hold no relays).
    pub fn covered(&self, c: GridCoord) -> bool {
        apart(c, self.base.position) * self.cell_m <= self.base.range_m + 1e-9
    }

    /// A station, depot or base station cell.
    pub fn infrastructure(&self, c: GridCoord) -> bool {
        c == self.base.position
            || self.stations.iter().any(|s| s.position == c)
            || self.depots.iter().any(|d| d.position == c)
    }

    pub fn travel_s(&self, r: usize, a: GridCoord, b: GridCoord) -> f64 {
        apart(a, b) * self.cell_m / self.robots[r].spec.max_speed_m_s.max(1e-9)
    }

    /// Energy for a stretch of driving, idle draw included, Wh.
    pub fn drive_wh(&self, r: usize, a: GridCoord, b: GridCoord) -> f64 {
        let spec = &self.robots[r].spec;
        apart(a, b) * self.cell_m * spec.drive_energy_wh_per_m
            + self.travel_s(r, a, b) * spec.idle_power_w / 3600.0
    }

    /// Duration and energy of `units` of work with a capability.
    pub fn work(&self, r: usize, name: CapabilityName, units: f64) -> (f64, f64) {
        let Some(c) = self.cap(r, name) else {
            return (f64::INFINITY, f64::INFINITY);
        };
        let s = (units / c.rate).max(MIN_WORK_S);
        let idle = self.robots[r].spec.idle_power_w;
        (s, units * c.energy_wh_per_unit + s * idle / 3600.0)
    }

    pub fn nearest_station(&self, c: GridCoord) -> Option<&ChargeStation> {
        self.stations
            .iter()
            .min_by(|a, b| apart(c, a.position).total_cmp(&apart(c, b.position)))
    }

    pub fn nearest_depot(&self, c: GridCoord) -> Option<&Depot> {
        self.depots
            .iter()
            .min_by(|a, b| apart(c, a.position).total_cmp(&apart(c, b.position)))
    }

    /// The charge a robot must keep to get from `from` to the nearest
    /// station, with a margin for telemetry noise, Wh.
    pub fn reserve_wh(&self, r: usize, from: GridCoord) -> f64 {
        let home = self
            .nearest_station(from)
            .map_or(0.0, |s| self.drive_wh(r, from, s.position));
        1.5 * home + 0.04 * self.capacity_wh(r)
    }

    /// Whether the battery covers `wh` of work ending at `end`, and the trip
    /// home from there.
    pub fn affords(&self, r: usize, wh: f64, end: GridCoord) -> bool {
        self.battery(r) >= 1.25 * wh + self.reserve_wh(r, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_helpers() {
        let a = GridCoord { x: 1, y: 1 };
        let b = GridCoord { x: 4, y: 5 };
        assert_eq!(apart(a, b), 5.0);
        let area = GridRect {
            min: GridCoord { x: 2, y: 2 },
            max: GridCoord { x: 3, y: 4 },
        };
        assert_eq!(entry(a, &area), GridCoord { x: 2, y: 2 });
        assert_eq!(entry(b, &area), GridCoord { x: 3, y: 4 });
        assert_eq!(cells_of(&area).count(), 6);
        assert_eq!(bounds(cells_of(&area)), Some(area));
        assert_eq!(bounds(std::iter::empty()), None);
        assert_eq!(cell(key(b)), b);
    }
}
