//! The landing-pad goal as tasks, shared by every baseline.
//!
//! Each pad cell moves through survey → dig → fill (only if low) → grade
//! → compact → sinter → verify, one task at a time, so different cells can
//! be at different stages and every robot can work at once.
//!
//! - **Survey.** One scan of the pad, the berm and the ground around them
//!   gives elevations, densities and boulders.
//! - **Dig.** Every pad cell is dug to the target: high cells are cut, and
//!   every cell is cleared of boulders, which grading needs. Spoil fills the
//!   digger's payload, and the overflow is cast where it is needed: onto a
//!   low pad cell it suits, else onto the berm, else onto a stockpile cell
//!   beside the pad.
//! - **Fill.** A cell more than [`LOW_M`] below the target, once dug, gets
//!   hauls from the stockpiles until it is just below the target.
//! - **Grade, compact, sinter** each take one step per cell.
//! - **Verify.** A row whose cells are all finished is scanned. A scan that
//!   shows a finished cell out of tolerance, or a step between neighbours
//!   close to the slope limit, sends the cell back to be redone.
//! - **Berm.** It gets spoil cast from the pad. If that is not enough, each
//!   short berm cell is fed by digging a borrow pit at the nearest covered
//!   cell, casting the spoil onto it. A scan confirms it.
//!
//! Every task stays in the base station's coverage: scans are entered from
//! covered cells, and the berm, which may be out of coverage, only ever
//! receives cast spoil. The plan keeps a ledger of material in flight, so
//! two digs never overfill one cell.

use super::world::{Key, World, apart, bounds, cell, cells_of, entry, key};
use arcadia_protocol::*;
use std::collections::BTreeMap;

/// A pad cell lower than this below the target gets fill before grading, m.
/// Grading only levels within 0.15 m (ArcadiaBench's documented window).
pub const LOW_M: f64 = 0.10;
/// Fill aims this far below the target, m.
const AIM_M: f64 = 0.03;
/// A pad cell higher than this above the target is dug again, m.
const HIGH_M: f64 = 0.10;
/// Berm cells are raised this far above the required height, m.
const BERM_MARGIN_M: f64 = 0.08;
/// A scanned berm cell counts as complete this far above the required height, m.
const BERM_OK_M: f64 = 0.015;
/// Most dig spoil cast onto one stockpile cell, kg.
const PILE_CAP_KG: f64 = 1500.0;
/// Loose material assumed on a stockpile cell before it is scanned, kg.
const NATIVE_GUESS_KG: f64 = 500.0;
/// In a recheck, neighbouring pad cells that scan further apart than this
/// are redone, m (2° over a 1 m cell is 0.035 m).
const STEP_LIMIT_M: f64 = 0.035;
/// In a recheck, finished cells that scan further than this from the
/// target are redone, m. Scans read elevation with 1 cm of noise, so
/// ordinary verification accepts the full tolerance.
const SUSPECT_M: f64 = 0.03;
/// Smallest fill worth a haul, kg.
const MIN_HAUL_KG: f64 = 30.0;
/// Pause before a task is retried after a failure, s.
const RETRY_S: f64 = 30.0;
/// Pause between whole-site rechecks, s.
const RECHECK_S: f64 = 3600.0;

/// Where a pad cell is in its pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Survey,
    Dig,
    Fill,
    Grade,
    Compact,
    Sinter,
    Done,
}

/// A unit of work.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Work {
    /// The first scan of the pad, the berm and the ground around them.
    Survey(GridRect),
    /// Rescan a pad cell whose knowledge is in doubt.
    Rescan(GridCoord),
    /// Scan finished cells to verify them.
    Verify(GridRect),
    /// Scan the berm to confirm it is complete.
    Check(GridRect),
    /// Scan everything when all looks finished but the goal is not complete.
    Recheck(GridRect),
    Dig(GridCoord),
    Fill(GridCoord),
    Grade(GridCoord),
    Compact(GridCoord),
    Sinter(GridCoord),
    /// Dig a borrow pit, casting the spoil onto a berm cell.
    Borrow {
        berm: GridCoord,
        pit: GridCoord,
    },
}

impl Work {
    /// The capability the work needs.
    pub fn capability(&self) -> CapabilityName {
        use CapabilityName as C;
        match self {
            Work::Survey(_)
            | Work::Rescan(_)
            | Work::Verify(_)
            | Work::Check(_)
            | Work::Recheck(_) => C::Scan,
            Work::Dig(_) | Work::Borrow { .. } => C::Dig,
            Work::Fill(_) => C::Haul,
            Work::Grade(_) => C::Grade,
            Work::Compact(_) => C::Compact,
            Work::Sinter(_) => C::Sinter,
        }
    }

    /// Priority class, most urgent first: scans that unblock or verify
    /// work, then the pipeline from its downstream end, then the berm.
    pub fn rank(&self) -> u8 {
        match self {
            Work::Rescan(_) => 0,
            Work::Survey(_) => 1,
            Work::Verify(_) | Work::Check(_) | Work::Recheck(_) => 2,
            Work::Sinter(_) => 3,
            Work::Compact(_) => 4,
            Work::Grade(_) => 5,
            Work::Fill(_) => 6,
            Work::Dig(_) => 7,
            Work::Borrow { .. } => 8,
        }
    }

    /// The pad cell whose pipeline the work advances, if any.
    fn pad_cell(&self) -> Option<GridCoord> {
        match *self {
            Work::Rescan(c)
            | Work::Dig(c)
            | Work::Fill(c)
            | Work::Grade(c)
            | Work::Compact(c)
            | Work::Sinter(c) => Some(c),
            _ => None,
        }
    }

    fn scan_area(&self) -> Option<GridRect> {
        match *self {
            Work::Survey(a) | Work::Verify(a) | Work::Check(a) | Work::Recheck(a) => Some(a),
            Work::Rescan(c) => Some(GridRect { min: c, max: c }),
            _ => None,
        }
    }
}

/// A task: work to be done, numbered in the order it was created.
#[derive(Debug, Clone)]
pub struct Task {
    pub id: u64,
    pub work: Work,
    /// A command for it is running.
    pub taken: bool,
    /// Not before this time, after a failure.
    pub not_before: f64,
}

/// What a task's command holds in the plan's ledger until it ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hold {
    None,
    /// Dig spoil expected to overflow onto a cell, kg.
    Cast {
        to: Key,
        kg: f64,
    },
    /// A haul: `load` kg taken from `from`, `kg` dumped on `to`.
    Haul {
        from: Key,
        to: Key,
        load: f64,
        kg: f64,
    },
    /// A scan, issued at this time: it verifies cells that last changed earlier.
    Scan {
        issued: f64,
    },
}

/// How a task's command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Completed,
    /// It failed or was refused for a reason that says nothing about the
    /// plan (a fault, the robot being busy): try again later.
    Retry,
    /// The ground is not what the plan thought: rescan the cell, or for a
    /// haul, stop counting on its source.
    Wrong,
}

/// A task's cost for one robot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    /// Travel plus work, s.
    pub secs: f64,
    /// Energy, Wh.
    pub wh: f64,
    /// Where the robot ends up.
    pub end: GridCoord,
}

#[derive(Debug, Clone)]
struct PadCell {
    stage: Stage,
    elev: f64,
    density: f64,
    boulders_m3: f64,
    /// Material in flight to this cell (casts and hauls), kg.
    incoming_kg: f64,
    incoming: u32,
    /// A task's command on this cell is running.
    busy: bool,
    /// Rescan before more work: a command found the ground other than modelled.
    stale: bool,
    /// When the cell last changed, as far as the plan knows.
    changed_t: f64,
    verified: bool,
    sweep: usize,
}

#[derive(Debug, Clone)]
struct BermCell {
    /// Elevation at the start of the run, once scanned.
    base: Option<f64>,
    elev: f64,
    density: f64,
    incoming_kg: f64,
    incoming: u32,
    pit: Option<Key>,
}

#[derive(Debug, Clone)]
struct Pile {
    /// Material that can be loaded here, kg.
    loadable_kg: f64,
    /// Spoil cast here, landed or in flight, kg.
    cast_kg: f64,
}

#[derive(Debug, Clone)]
struct Pit {
    elev: f64,
    density: f64,
    busy: bool,
    /// The elevation a running borrow dig cuts it to.
    cutting_to: Option<f64>,
}

/// The plan for the landing pad.
#[derive(Debug, Clone)]
pub struct Plan {
    pub pad: LandingPadSpec,
    cells: BTreeMap<Key, PadCell>,
    berm: BTreeMap<Key, BermCell>,
    pits: BTreeMap<Key, Pit>,
    piles: BTreeMap<Key, Pile>,
    tasks: BTreeMap<u64, Task>,
    next_id: u64,
    survey: GridRect,
    berm_area: Option<GridRect>,
    surveyed: bool,
    berm_ok: bool,
    recheck_after: f64,
}

/// The pad's cells in sweep order: rows from the corner nearest `from`,
/// alternating direction, so consecutive cells are neighbours.
pub fn sweep(area: &GridRect, from: GridCoord) -> Vec<GridCoord> {
    let corners = [
        GridCoord {
            x: area.min.x,
            y: area.min.y,
        },
        GridCoord {
            x: area.max.x,
            y: area.min.y,
        },
        GridCoord {
            x: area.min.x,
            y: area.max.y,
        },
        GridCoord {
            x: area.max.x,
            y: area.max.y,
        },
    ];
    let start = corners
        .into_iter()
        .min_by(|a, b| apart(from, *a).total_cmp(&apart(from, *b)))
        .expect("four corners");
    let rows: Vec<u32> = if start.y == area.min.y {
        (area.min.y..=area.max.y).collect()
    } else {
        (area.min.y..=area.max.y).rev().collect()
    };
    let mut out = Vec::new();
    for (i, y) in rows.into_iter().enumerate() {
        let east = (start.x == area.min.x) == (i % 2 == 0);
        let xs: Vec<u32> = if east {
            (area.min.x..=area.max.x).collect()
        } else {
            (area.min.x..=area.max.x).rev().collect()
        };
        out.extend(xs.into_iter().map(|x| GridCoord { x, y }));
    }
    out
}

impl Plan {
    /// A plan for `pad` on the site `world` knows, surveyed first.
    pub fn new(pad: LandingPadSpec, world: &World) -> Self {
        let in_pad = |c: GridCoord| {
            (pad.area.min.x..=pad.area.max.x).contains(&c.x)
                && (pad.area.min.y..=pad.area.max.y).contains(&c.y)
        };
        // The crew's centre, where the sweep starts.
        let placed: Vec<GridCoord> = (0..world.robots.len())
            .filter(|&r| world.state(r).is_some())
            .map(|r| world.pos(r))
            .collect();
        let from = if placed.is_empty() {
            world.stations.first().map_or(pad.area.min, |s| s.position)
        } else {
            let n = placed.len() as f64;
            let mx = placed.iter().map(|c| f64::from(c.x)).sum::<f64>() / n;
            let my = placed.iter().map(|c| f64::from(c.y)).sum::<f64>() / n;
            GridCoord {
                x: mx.round() as u32,
                y: my.round() as u32,
            }
        };
        let prior = |c: GridCoord| {
            world
                .prior
                .get(&key(c))
                .copied()
                .unwrap_or(pad.target_elevation_m)
        };
        let cells: BTreeMap<Key, PadCell> = sweep(&pad.area, from)
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    key(c),
                    PadCell {
                        stage: Stage::Survey,
                        elev: prior(c),
                        density: 1550.0,
                        boulders_m3: 0.0,
                        incoming_kg: 0.0,
                        incoming: 0,
                        busy: false,
                        stale: false,
                        changed_t: 0.0,
                        verified: false,
                        sweep: i,
                    },
                )
            })
            .collect();
        let berm_cells: Vec<GridCoord> = pad
            .berm
            .as_ref()
            .map(|b| b.cells.clone())
            .unwrap_or_default();
        let is_berm = |c: GridCoord| berm_cells.contains(&c);

        // A borrow pit per berm cell: the nearest covered cell that is not
        // pad, berm or infrastructure.
        let mut pits: BTreeMap<Key, Pit> = BTreeMap::new();
        let mut berm = BTreeMap::new();
        for &b in &berm_cells {
            let pit = (0..world.height)
                .flat_map(|y| (0..world.width).map(move |x| GridCoord { x, y }))
                .filter(|&c| {
                    world.covered(c)
                        && !in_pad(c)
                        && !is_berm(c)
                        && !world.infrastructure(c)
                        && !pits.contains_key(&key(c))
                })
                .min_by(|a, c| apart(b, *a).total_cmp(&apart(b, *c)));
            if let Some(p) = pit {
                pits.insert(
                    key(p),
                    Pit {
                        elev: prior(p),
                        density: 1550.0,
                        busy: false,
                        cutting_to: None,
                    },
                );
            }
            berm.insert(
                key(b),
                BermCell {
                    base: None,
                    elev: prior(b),
                    density: 1550.0,
                    incoming_kg: 0.0,
                    incoming: 0,
                    pit: pit.map(key),
                },
            );
        }

        // Stockpiles: the covered ring two cells deep around the pad.
        let (lo_x, lo_y) = (
            pad.area.min.x.saturating_sub(2),
            pad.area.min.y.saturating_sub(2),
        );
        let ring = GridRect {
            min: GridCoord { x: lo_x, y: lo_y },
            max: GridCoord {
                x: (pad.area.max.x + 2).min(world.width.saturating_sub(1)),
                y: (pad.area.max.y + 2).min(world.height.saturating_sub(1)),
            },
        };
        let piles: BTreeMap<Key, Pile> = cells_of(&ring)
            .filter(|&c| {
                !in_pad(c)
                    && !is_berm(c)
                    && !pits.contains_key(&key(c))
                    && world.covered(c)
                    && !world.infrastructure(c)
            })
            .map(|c| {
                (
                    key(c),
                    Pile {
                        loadable_kg: NATIVE_GUESS_KG,
                        cast_kg: 0.0,
                    },
                )
            })
            .collect();

        let corners = [pad.area.min, pad.area.max];
        let survey = bounds(
            corners
                .into_iter()
                .chain(berm_cells.iter().copied())
                .chain(pits.keys().map(|&k| cell(k)))
                .chain(piles.keys().map(|&k| cell(k))),
        )
        .expect("the pad has cells");
        let berm_area = bounds(
            berm_cells
                .iter()
                .copied()
                .chain(pits.keys().map(|&k| cell(k))),
        );
        let mut plan = Self {
            pad,
            cells,
            berm,
            pits,
            piles,
            tasks: BTreeMap::new(),
            next_id: 1,
            survey,
            berm_area,
            surveyed: false,
            berm_ok: berm_cells.is_empty(),
            recheck_after: 0.0,
        };
        plan.add(Work::Survey(survey));
        plan
    }

    fn add(&mut self, work: Work) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.insert(
            id,
            Task {
                id,
                work,
                taken: false,
                not_before: 0.0,
            },
        );
        id
    }

    /// Add `work` unless the same work is already planned.
    fn ensure(&mut self, work: Work) {
        if !self.tasks.values().any(|t| t.work == work) {
            self.add(work);
        }
    }

    pub fn task(&self, id: u64) -> Option<&Task> {
        self.tasks.get(&id)
    }

    fn target(&self) -> f64 {
        self.pad.target_elevation_m
    }

    /// Fill a pad cell still needs, beyond what is already on its way, kg.
    fn need_kg(&self, c: &PadCell) -> f64 {
        let after = c.elev + c.incoming_kg / c.density;
        if after >= self.target() - LOW_M {
            0.0
        } else {
            (self.target() - AIM_M - after) * c.density
        }
    }

    /// The most material a pad cell can take and still be graded, kg.
    fn room_kg(&self, c: &PadCell) -> f64 {
        let after = c.elev + c.incoming_kg / c.density;
        ((self.target() + HIGH_M - after) * c.density).max(0.0)
    }

    /// Material a berm cell still needs, beyond what is on its way, kg.
    fn berm_need_kg(&self, b: &BermCell) -> f64 {
        let height = self.pad.berm.as_ref().map_or(0.0, |s| s.height_m);
        match b.base {
            Some(base) => {
                ((base + height + BERM_MARGIN_M - b.elev) * b.density - b.incoming_kg).max(0.0)
            }
            None => 0.0,
        }
    }

    /// Tasks that can start now, unsorted.
    pub fn ready(&self, now: f64) -> Vec<u64> {
        self.tasks
            .keys()
            .copied()
            .filter(|&id| self.can_start(id, now))
            .collect()
    }

    /// Whether task `id` can start now: not taken, not backing off, and its
    /// cell free (nothing landing on it, no other work on it).
    pub fn can_start(&self, id: u64, now: f64) -> bool {
        self.tasks
            .get(&id)
            .is_some_and(|t| !t.taken && t.not_before <= now && self.startable(&t.work))
    }

    fn startable(&self, work: &Work) -> bool {
        match *work {
            Work::Fill(c) => self
                .cells
                .get(&key(c))
                .is_some_and(|p| !p.busy && self.need_kg(p) >= MIN_HAUL_KG),
            Work::Rescan(_) => true,
            Work::Borrow { pit, .. } => self.pits.get(&key(pit)).is_some_and(|p| !p.busy),
            w => match w.pad_cell() {
                // Nothing may land on a cell while it is worked.
                Some(c) => self
                    .cells
                    .get(&key(c))
                    .is_some_and(|p| !p.busy && !p.stale && p.incoming == 0),
                None => true,
            },
        }
    }

    /// Sort key: priority class, then the cell's place in the sweep, then age.
    pub fn priority(&self, id: u64) -> (u8, usize, u64) {
        let t = &self.tasks[&id];
        let sweep = t
            .work
            .pad_cell()
            .and_then(|c| self.cells.get(&key(c)))
            .map_or(0, |p| p.sweep);
        (t.work.rank(), sweep, id)
    }

    /// The cost of a task for robot `r`, or `None` if `r` cannot do it now.
    pub fn estimate(&self, id: u64, r: usize, world: &World) -> Option<Estimate> {
        let work = self.tasks.get(&id)?.work;
        let cap = work.capability();
        world.cap(r, cap)?;
        let pos = world.pos(r);
        let area_m2 = world.cell_m * world.cell_m;
        let at_cell = |c: GridCoord, units: f64| {
            let (s, wh) = world.work(r, cap, units);
            Some(Estimate {
                secs: world.travel_s(r, pos, c) + s,
                wh: world.drive_wh(r, pos, c) + wh,
                end: c,
            })
        };
        if let Some(area) = work.scan_area() {
            let e = entry(pos, &area);
            if !world.covered(e) {
                return None;
            }
            let units = cells_of(&area).count() as f64 * area_m2;
            return at_cell(e, units);
        }
        match work {
            Work::Dig(c) => {
                let p = self.cells.get(&key(c))?;
                at_cell(
                    c,
                    (p.elev - self.target()).max(0.0) * area_m2 + p.boulders_m3,
                )
            }
            Work::Borrow { berm, pit } => {
                let (target, _) = self.borrow_plan(r, berm, world)?;
                let depth = (self.pits.get(&key(pit))?.elev - target).max(0.0);
                at_cell(pit, depth * area_m2)
            }
            Work::Fill(c) => {
                let (from, load, kg) = self.haul_plan(r, c, world)?;
                let from = cell(from);
                let rate = world.cap(r, cap)?.rate;
                let work_s = (load + kg) / rate;
                let spec = &world.robots[r].spec;
                let e = world.cap(r, cap)?.energy_wh_per_unit;
                Some(Estimate {
                    secs: world.travel_s(r, pos, from) + world.travel_s(r, from, c) + work_s,
                    wh: world.drive_wh(r, pos, from)
                        + world.drive_wh(r, from, c)
                        + (load + kg) * e
                        + work_s * spec.idle_power_w / 3600.0,
                    end: c,
                })
            }
            Work::Grade(c) | Work::Compact(c) | Work::Sinter(c) => at_cell(c, area_m2),
            _ => None,
        }
    }

    /// The command for task `id` done by robot `r`, booking what it holds in
    /// the ledger and marking the task taken. `None` if `r` cannot do it now.
    pub fn command(&mut self, id: u64, r: usize, world: &World) -> Option<(Action, Hold)> {
        let work = self.tasks.get(&id)?.work;
        let issued = world.t;
        let target = self.target();
        let (action, hold) = match work {
            Work::Survey(area) | Work::Verify(area) | Work::Check(area) | Work::Recheck(area) => {
                (Action::Scan { area }, Hold::Scan { issued })
            }
            Work::Rescan(c) => (
                Action::Scan {
                    area: GridRect { min: c, max: c },
                },
                Hold::Scan { issued },
            ),
            Work::Dig(c) => {
                let p = self.cells.get(&key(c))?;
                let area_m2 = world.cell_m * world.cell_m;
                let spoil = (p.elev - target).max(0.0) * p.density * area_m2
                    + p.boulders_m3 * world.rock_kg_m3;
                let overflow = (spoil - world.room_kg(r)).max(0.0);
                let to = self.route(c, overflow);
                let hold = match to {
                    Some(to) if overflow > 1.0 => {
                        self.book(to, overflow);
                        Hold::Cast { to, kg: overflow }
                    }
                    _ => Hold::None,
                };
                (
                    Action::Dig {
                        area: GridRect { min: c, max: c },
                        target_elevation_m: target,
                        spoil_to: to.map(cell),
                    },
                    hold,
                )
            }
            Work::Borrow { berm, pit } => {
                let (pit_target, kg) = self.borrow_plan(r, berm, world)?;
                self.book(key(berm), kg);
                if let Some(p) = self.pits.get_mut(&key(pit)) {
                    p.busy = true;
                    p.cutting_to = Some(pit_target);
                }
                (
                    Action::Dig {
                        area: GridRect { min: pit, max: pit },
                        target_elevation_m: pit_target,
                        spoil_to: Some(berm),
                    },
                    Hold::Cast { to: key(berm), kg },
                )
            }
            Work::Fill(c) => {
                let (from, load, kg) = self.haul_plan(r, c, world)?;
                if let Some(p) = self.piles.get_mut(&from) {
                    p.loadable_kg -= load;
                }
                self.book(key(c), kg);
                (
                    Action::Haul {
                        from: cell(from),
                        to: c,
                        mass_kg: kg,
                    },
                    Hold::Haul {
                        from,
                        to: key(c),
                        load,
                        kg,
                    },
                )
            }
            Work::Grade(c) => (
                Action::Grade {
                    area: GridRect { min: c, max: c },
                    target_elevation_m: target,
                },
                Hold::None,
            ),
            Work::Compact(c) => (
                Action::Compact {
                    area: GridRect { min: c, max: c },
                },
                Hold::None,
            ),
            Work::Sinter(c) => (
                Action::Sinter {
                    area: GridRect { min: c, max: c },
                },
                Hold::None,
            ),
        };
        // Ground-changing work holds its cell; a fill or a rescan does not.
        if let Some(c) = work
            .pad_cell()
            .filter(|_| !matches!(work, Work::Fill(_) | Work::Rescan(_)))
            && let Some(p) = self.cells.get_mut(&key(c))
        {
            p.busy = true;
        }
        self.tasks.get_mut(&id)?.taken = true;
        Some((action, hold))
    }

    /// Where a dig's overflow goes: a low pad cell it suits, earliest in the
    /// sweep; else the berm cell that needs most; else the nearest
    /// stockpile with room.
    fn route(&self, dug: GridCoord, kg: f64) -> Option<Key> {
        if kg > 1.0 {
            let pad = self
                .cells
                .iter()
                .filter(|&(&k, p)| {
                    k != key(dug)
                        && matches!(p.stage, Stage::Dig | Stage::Fill)
                        && !p.busy
                        && !p.stale
                        && self.need_kg(p) > 0.0
                        && kg <= self.room_kg(p)
                })
                .min_by_key(|(_, p)| p.sweep)
                .map(|(&k, _)| k);
            if pad.is_some() {
                return pad;
            }
            let berm = self
                .berm
                .iter()
                .map(|(&k, b)| (k, self.berm_need_kg(b)))
                .filter(|&(_, need)| need > 0.0)
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
                .map(|(k, _)| k);
            if berm.is_some() {
                return berm;
            }
        }
        let near = |a: &(&Key, &Pile), b: &(&Key, &Pile)| {
            apart(dug, cell(*a.0)).total_cmp(&apart(dug, cell(*b.0)))
        };
        self.piles
            .iter()
            .filter(|(_, p)| p.cast_kg + kg <= PILE_CAP_KG)
            .min_by(near)
            .or_else(|| self.piles.iter().min_by(near))
            .map(|(&k, _)| k)
    }

    /// Book `kg` of material on its way to a cell.
    fn book(&mut self, to: Key, kg: f64) {
        if let Some(p) = self.cells.get_mut(&to) {
            p.incoming_kg += kg;
            p.incoming += 1;
        } else if let Some(b) = self.berm.get_mut(&to) {
            b.incoming_kg += kg;
            b.incoming += 1;
        } else if let Some(p) = self.piles.get_mut(&to) {
            p.cast_kg += kg;
        }
    }

    /// Material booked to a cell has landed (`landed`) or will not come.
    fn unbook(&mut self, to: Key, kg: f64, landed: bool, t: f64) {
        if let Some(p) = self.cells.get_mut(&to) {
            p.incoming_kg = (p.incoming_kg - kg).max(0.0);
            p.incoming = p.incoming.saturating_sub(1);
            if p.incoming == 0 {
                p.incoming_kg = 0.0;
            }
            if landed {
                p.elev += kg / p.density;
                p.changed_t = t;
                p.verified = false;
            }
            self.settle(to);
        } else if let Some(b) = self.berm.get_mut(&to) {
            b.incoming_kg = (b.incoming_kg - kg).max(0.0);
            b.incoming = b.incoming.saturating_sub(1);
            if b.incoming == 0 {
                b.incoming_kg = 0.0;
            }
            if landed {
                b.elev += kg / b.density;
                self.berm_ok = false;
            }
        } else if let Some(p) = self.piles.get_mut(&to) {
            if landed {
                p.loadable_kg += kg;
            } else {
                p.cast_kg = (p.cast_kg - kg).max(0.0);
            }
        }
    }

    /// A haul into pad cell `c` by robot `r`: the source, the mass to load
    /// there and the mass dumped. A robot dumps all it carries, so a payload
    /// too big for the cell rules it out.
    fn haul_plan(&self, r: usize, c: GridCoord, world: &World) -> Option<(Key, f64, f64)> {
        let p = self.cells.get(&key(c))?;
        if p.busy {
            return None;
        }
        let need = self.need_kg(p);
        if need < MIN_HAUL_KG {
            return None;
        }
        let capacity = world.cap(r, CapabilityName::Haul)?.capacity?;
        let payload = world.payload(r);
        let kg = need.min(capacity).max(payload);
        if kg > self.room_kg(p) {
            return None;
        }
        let load = kg - payload;
        if load < 1.0 {
            return Some((key(world.pos(r)), 0.0, kg));
        }
        let pos = world.pos(r);
        self.piles
            .iter()
            .filter(|(_, pile)| pile.loadable_kg >= load)
            .min_by(|a, b| {
                let via = |k: &Key| apart(pos, cell(*k)) + apart(cell(*k), c);
                via(a.0).total_cmp(&via(b.0))
            })
            .map(|(&k, _)| (k, load, kg))
    }

    /// A borrow dig for berm cell `b` by robot `r`: the pit's target
    /// elevation, and the spoil expected on the berm, kg.
    fn borrow_plan(&self, r: usize, b: GridCoord, world: &World) -> Option<(f64, f64)> {
        let berm = self.berm.get(&key(b))?;
        let need = self.berm_need_kg(berm);
        if need < MIN_HAUL_KG {
            return None;
        }
        let pit = self.pits.get(&berm.pit?)?;
        if pit.busy {
            return None;
        }
        let area_m2 = world.cell_m * world.cell_m;
        let depth = (need + world.room_kg(r)) / (pit.density * area_m2) + 0.02;
        Some((pit.elev - depth, need))
    }

    /// Settle the ledger and the pipeline when task `id`'s command ends.
    pub fn finish(&mut self, id: u64, hold: Hold, end: End, t: f64) {
        let Some(work) = self.tasks.get(&id).map(|t| t.work) else {
            return;
        };
        let target = self.target();
        if let Some(c) = work.pad_cell()
            && let Some(p) = self.cells.get_mut(&key(c))
        {
            p.busy = false;
        }
        if let Work::Borrow { pit, .. } = work
            && let Some(p) = self.pits.get_mut(&key(pit))
        {
            p.busy = false;
            let to = p.cutting_to.take();
            if end == End::Completed {
                p.elev = to.map_or(p.elev, |to| p.elev.min(to));
            }
        }
        let done = end == End::Completed;
        match hold {
            Hold::Cast { to, kg } => self.unbook(to, kg, done, t),
            Hold::Haul { from, to, kg, .. } => {
                if end == End::Wrong
                    && let Some(p) = self.piles.get_mut(&from)
                {
                    p.loadable_kg = 0.0;
                }
                self.unbook(to, kg, done, t);
            }
            Hold::None | Hold::Scan { .. } => {}
        }
        if !done {
            if end == End::Wrong
                && let Some(c) = work.pad_cell().filter(|_| !matches!(work, Work::Fill(_)))
            {
                self.tasks.remove(&id);
                if let Some(p) = self.cells.get_mut(&key(c)) {
                    p.stale = true;
                }
                self.ensure(Work::Rescan(c));
                return;
            }
            if let Some(task) = self.tasks.get_mut(&id) {
                task.taken = false;
                task.not_before = t + RETRY_S;
            }
            return;
        }
        self.tasks.remove(&id);
        match work {
            Work::Dig(c) => {
                if let Some(p) = self.cells.get_mut(&key(c)) {
                    p.elev = p.elev.min(target);
                    p.boulders_m3 = 0.0;
                    p.changed_t = t;
                    p.stage = Stage::Fill;
                }
                self.settle(key(c));
            }
            Work::Borrow { .. } => {}
            Work::Fill(c) => self.settle(key(c)),
            Work::Grade(c) => self.advance(c, target, Stage::Compact, t),
            Work::Compact(c) => {
                let elev = self.cells.get(&key(c)).map_or(target, |p| p.elev) - 0.01;
                // A pad that needs no stabilizing is finished once compacted.
                let next = if self.pad.stabilize {
                    Stage::Sinter
                } else {
                    Stage::Done
                };
                self.advance(c, elev, next, t);
            }
            Work::Sinter(c) => {
                let elev = self.cells.get(&key(c)).map_or(target, |p| p.elev);
                self.advance(c, elev, Stage::Done, t);
            }
            Work::Rescan(c) => self.settle(key(c)),
            Work::Survey(_) | Work::Verify(_) | Work::Check(_) | Work::Recheck(_) => {}
        }
    }

    /// A cell's step finished: record its new elevation and move it on.
    fn advance(&mut self, c: GridCoord, elev: f64, next: Stage, t: f64) {
        if let Some(p) = self.cells.get_mut(&key(c)) {
            p.elev = elev;
            p.changed_t = t;
            p.verified = false;
            p.stage = next;
        }
        self.settle(key(c));
    }

    /// Plan a pad cell's next task from what is known of it.
    fn settle(&mut self, k: Key) {
        let target = self.target();
        let Some(p) = self.cells.get(&k).cloned() else {
            return;
        };
        if p.busy || p.stale || p.incoming > 0 {
            return;
        }
        let c = cell(k);
        let work = match p.stage {
            Stage::Survey => return,
            Stage::Dig => Work::Dig(c),
            // After digging or filling: dig again, fill, or grade.
            Stage::Fill | Stage::Grade => {
                let stage = if p.elev > target + HIGH_M {
                    Stage::Dig
                } else if p.elev < target - LOW_M {
                    Stage::Fill
                } else {
                    Stage::Grade
                };
                self.cells.get_mut(&k).expect("pad cell").stage = stage;
                match stage {
                    Stage::Dig => Work::Dig(c),
                    Stage::Fill => Work::Fill(c),
                    _ => Work::Grade(c),
                }
            }
            Stage::Compact => Work::Compact(c),
            Stage::Sinter => Work::Sinter(c),
            Stage::Done => {
                self.verify_when_ready(c);
                return;
            }
        };
        // Drop any stale task for this cell before planning the next one.
        let old: Vec<u64> = self
            .tasks
            .values()
            .filter(|task| !task.taken && task.work.pad_cell() == Some(c))
            .filter(|task| !matches!(task.work, Work::Rescan(_)))
            .map(|task| task.id)
            .collect();
        for id in old {
            if self.tasks[&id].work != work {
                self.tasks.remove(&id);
            }
        }
        self.ensure(work);
    }

    /// Once a whole row is finished, scan it; once every cell is finished,
    /// scan what is left unverified.
    fn verify_when_ready(&mut self, c: GridCoord) {
        let row = GridRect {
            min: GridCoord {
                x: self.pad.area.min.x,
                y: c.y,
            },
            max: GridCoord {
                x: self.pad.area.max.x,
                y: c.y,
            },
        };
        let finished = |p: &PadCell| p.stage == Stage::Done && !p.busy && p.incoming == 0;
        let row_done = cells_of(&row).all(|x| self.cells.get(&key(x)).is_some_and(finished));
        let row_unverified =
            cells_of(&row).any(|x| self.cells.get(&key(x)).is_some_and(|p| !p.verified));
        if row_done && row_unverified {
            self.ensure(Work::Verify(row));
        }
        if self.cells.values().all(finished) {
            let pending: Vec<GridRect> = self
                .tasks
                .values()
                .filter_map(|t| match t.work {
                    Work::Verify(a) => Some(a),
                    _ => None,
                })
                .collect();
            let left = bounds(
                self.cells
                    .iter()
                    .filter(|(_, p)| !p.verified)
                    .map(|(&k, _)| cell(k))
                    .filter(|&x| !pending.iter().any(|a| in_rect(x, a))),
            );
            if let Some(area) = left {
                self.ensure(Work::Verify(area));
            }
        }
    }

    /// Take in the scan task `id` issued at `issued`; `world` already holds
    /// its cells.
    pub fn scanned(
        &mut self,
        id: u64,
        cells: &[CellObservation],
        issued: f64,
        t: f64,
        world: &World,
    ) {
        let first = !self.surveyed;
        let recheck = self
            .tasks
            .get(&id)
            .is_some_and(|task| matches!(task.work, Work::Recheck(_)));
        let tol = self.pad.surface_tolerance_m;
        let target = self.target();
        let required = if self.pad.stabilize {
            Surface::Stabilized
        } else {
            Surface::Compacted
        };
        let area_m2 = world.cell_m * world.cell_m;
        let mut touched = Vec::new();
        for obs in cells {
            let k = key(obs.at);
            let Some(s) = world.seen.get(&k) else {
                continue;
            };
            if let Some(p) = self.cells.get_mut(&k) {
                if first || p.stale {
                    p.elev = s.elevation_m;
                    p.density = s.density_kg_m3;
                    p.boulders_m3 = s.boulders_m3;
                    if p.stale {
                        p.stale = false;
                        p.stage = restage(s.surface, self.pad.stabilize);
                        p.verified = false;
                        touched.push(k);
                    }
                } else if p.stage == Stage::Done && !p.busy && p.incoming == 0 {
                    p.elev = s.elevation_m;
                    let ok = s.surface == required && (s.elevation_m - target).abs() <= tol;
                    if ok {
                        if issued >= p.changed_t {
                            p.verified = true;
                        }
                    } else {
                        p.stage = restage(s.surface, self.pad.stabilize);
                        if p.stage == Stage::Done {
                            p.stage = Stage::Dig;
                        }
                        p.verified = false;
                        p.changed_t = t;
                        touched.push(k);
                    }
                } else if !p.busy && p.incoming == 0 && p.stage <= Stage::Fill {
                    p.elev = s.elevation_m;
                    p.boulders_m3 = s.boulders_m3;
                }
            }
            if let Some(b) = self.berm.get_mut(&k) {
                if b.base.is_none() {
                    b.base = Some(s.elevation_m);
                }
                b.density = s.density_kg_m3;
                if b.incoming == 0 {
                    b.elev = s.elevation_m;
                }
            }
            if let Some(p) = self.pits.get_mut(&k)
                && !p.busy
            {
                p.elev = s.elevation_m;
                p.density = s.density_kg_m3;
            }
            if first && let Some(p) = self.piles.get_mut(&k) {
                p.loadable_kg = s.depth_m * s.density_kg_m3 * area_m2;
            }
        }
        if first && cells_of(&self.pad.area).all(|c| world.seen.contains_key(&key(c))) {
            self.surveyed = true;
            let order: Vec<(usize, Key)> = self.cells.iter().map(|(&k, p)| (p.sweep, k)).collect();
            let mut order = order;
            order.sort_unstable();
            for (_, k) in order {
                if let Some(p) = self.cells.get_mut(&k) {
                    p.stage = Stage::Dig;
                }
                self.settle(k);
            }
        }
        if recheck {
            touched.extend(self.suspects(cells));
        }
        touched.sort_unstable();
        touched.dedup();
        for k in touched {
            self.settle(k);
        }
        // A berm scan after the last deposit confirms it, or shows what is short.
        let height = self.pad.berm.as_ref().map_or(0.0, |b| b.height_m);
        let scanned_all = self
            .berm
            .keys()
            .all(|k| cells.iter().any(|o| key(o.at) == *k));
        if self.surveyed && !self.berm.is_empty() && scanned_all {
            self.berm_ok = self.berm.values().all(|b| {
                b.incoming == 0
                    && b.base
                        .is_some_and(|base| b.elev >= base + height + BERM_OK_M)
            });
        }
        // Verification may be due for rows the scan finished checking.
        let done_rows: Vec<GridCoord> = cells
            .iter()
            .map(|o| o.at)
            .filter(|c| {
                self.cells
                    .get(&key(*c))
                    .is_some_and(|p| p.stage == Stage::Done)
            })
            .collect();
        for c in done_rows {
            self.verify_when_ready(c);
        }
    }

    /// After a recheck: finished cells that scan suspiciously far from the
    /// target, and the worse of each pair of neighbours further apart than
    /// the slope allows, are redone. The worst cell is redone in any case,
    /// so every recheck changes something.
    fn suspects(&mut self, cells: &[CellObservation]) -> Vec<Key> {
        let target = self.target();
        let mut out = Vec::new();
        let worst = cells
            .iter()
            .map(|o| key(o.at))
            .filter(|k| self.cells.get(k).is_some_and(|p| p.stage == Stage::Done))
            .max_by(|a, b| {
                let off = |k: &Key| (self.cells[k].elev - target).abs();
                off(a).total_cmp(&off(b)).then(b.cmp(a))
            });
        out.extend(worst);
        for obs in cells {
            let k = key(obs.at);
            let Some(p) = self.cells.get(&k) else {
                continue;
            };
            if p.stage != Stage::Done {
                continue;
            }
            if (p.elev - target).abs() > SUSPECT_M {
                out.push(k);
            }
            for n in [(k.0 + 1, k.1), (k.0, k.1 + 1)] {
                let Some(q) = self.cells.get(&n) else {
                    continue;
                };
                if q.stage != Stage::Done || (p.elev - q.elev).abs() <= STEP_LIMIT_M {
                    continue;
                }
                let worse = if (p.elev - target).abs() >= (q.elev - target).abs() {
                    k
                } else {
                    n
                };
                out.push(worse);
            }
        }
        for &k in &out {
            if let Some(p) = self.cells.get_mut(&k) {
                p.stage = Stage::Dig;
                p.verified = false;
            }
        }
        out
    }

    /// Plan what the pipeline cannot: borrow digs for the berm once the pad
    /// is dug, a berm check once nothing more is on its way, and a recheck
    /// of the whole pad when everything looks finished but the goal is not
    /// complete.
    pub fn review(&mut self, now: f64) {
        if !self.surveyed {
            return;
        }
        let digging = self.cells.values().any(|p| p.stage <= Stage::Dig);
        let short: Vec<(Key, Option<Key>)> = self
            .berm
            .iter()
            .filter(|(_, b)| self.berm_need_kg(b) >= MIN_HAUL_KG)
            .map(|(&k, b)| (k, b.pit))
            .collect();
        if !digging {
            for (b, pit) in &short {
                if let Some(pit) = pit {
                    self.ensure(Work::Borrow {
                        berm: cell(*b),
                        pit: cell(*pit),
                    });
                }
            }
        }
        let checking = self
            .tasks
            .values()
            .any(|t| matches!(t.work, Work::Check(_)));
        let arriving = self.berm.values().any(|b| b.incoming > 0);
        if !self.berm_ok
            && short.is_empty()
            && !arriving
            && !checking
            && let Some(area) = self.berm_area
        {
            self.ensure(Work::Check(area));
        }
        let all_done = self
            .cells
            .values()
            .all(|p| p.stage == Stage::Done && p.verified && !p.busy);
        if all_done && self.berm_ok && self.tasks.is_empty() && now >= self.recheck_after {
            self.recheck_after = now + RECHECK_S;
            self.add(Work::Recheck(self.survey));
        }
    }
}

fn in_rect(c: GridCoord, a: &GridRect) -> bool {
    (a.min.x..=a.max.x).contains(&c.x) && (a.min.y..=a.max.y).contains(&c.y)
}

/// The stage a pad cell is at, from its scanned surface.
fn restage(surface: Surface, stabilize: bool) -> Stage {
    match surface {
        // A surface from a later protocol version is treated as untouched
        // ground: digging is legal from any state.
        Surface::Natural | Surface::Stockpile | Surface::Berm | Surface::Unknown => Stage::Dig,
        Surface::Cleared => Stage::Fill,
        Surface::Graded => Stage::Compact,
        Surface::Compacted if stabilize => Stage::Sinter,
        Surface::Compacted | Surface::Stabilized => Stage::Done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sweep_starts_at_the_nearest_corner_and_snakes() {
        let area = GridRect {
            min: GridCoord { x: 2, y: 2 },
            max: GridCoord { x: 4, y: 3 },
        };
        let order = sweep(&area, GridCoord { x: 0, y: 0 });
        let xy: Vec<(u32, u32)> = order.iter().map(|c| (c.x, c.y)).collect();
        assert_eq!(xy, [(2, 2), (3, 2), (4, 2), (4, 3), (3, 3), (2, 3)]);
        let order = sweep(&area, GridCoord { x: 9, y: 9 });
        assert_eq!((order[0].x, order[0].y), (4, 3));
        assert_eq!(order.len(), 6);
    }

    #[test]
    fn restage_follows_the_surface() {
        assert_eq!(restage(Surface::Natural, true), Stage::Dig);
        assert_eq!(restage(Surface::Cleared, true), Stage::Fill);
        assert_eq!(restage(Surface::Graded, true), Stage::Compact);
        assert_eq!(restage(Surface::Compacted, true), Stage::Sinter);
        assert_eq!(restage(Surface::Compacted, false), Stage::Done);
        assert_eq!(restage(Surface::Stabilized, true), Stage::Done);
    }
}
