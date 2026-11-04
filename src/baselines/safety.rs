//! The safety behaviours every baseline shares, whatever its allocation.
//! They run before allocation, on the robots free to take a command:
//!
//! 1. **Rescue disabled robots**, robot by robot:
//!    - stuck or failed on a depot's cell: `service` in place;
//!    - offline on a station's cell: `charge` in place;
//!    - stuck: field `repair` by a free repair-capable robot, else `tow`
//!      to the nearest depot;
//!    - failed with only field-repairable disabling faults: field
//!      `repair` (waiting for a repairer if all are busy), else `tow` to
//!      the nearest depot for service;
//!    - offline: a free mobile charger drives over and charges it (once
//!      per charger), else `tow` to the nearest station.
//!
//!    A robot nobody can help is left alone, which drops it from the work.
//! 2. **Repair degraded faults** in the field when a repairer and the
//!    robot are both free.
//! 3. **Charge when low**: a free robot below [`LOW_FRAC`] of its battery
//!    charges to full at the station with the shortest trip and queue.
//!
//! After allocation, a robot that could do waiting work but cannot afford
//! any of it also charges.

use super::plan::{Hold, Plan};
use super::world::{World, apart};
use super::{Flight, Job, Order};
use arcadia_protocol::*;
use std::collections::{BTreeMap, BTreeSet};

/// A free robot below this fraction of its battery charges before working.
pub const LOW_FRAC: f64 = 0.2;
/// A helper needs this fraction of its battery to take on a rescue.
const HELPER_FRAC: f64 = 0.4;
/// Assumed wait per robot ahead in a full station queue, s.
const QUEUE_S: f64 = 3600.0;
/// Robots reach each other within this many cells.
const REACH: f64 = 1.5;

/// The free robots of one decision and the orders given so far.
pub struct Crew<'a> {
    world: &'a World,
    flights: &'a BTreeMap<String, Flight>,
    pub free: Vec<usize>,
    pub orders: Vec<Order>,
}

impl<'a> Crew<'a> {
    pub fn new(world: &'a World, flights: &'a BTreeMap<String, Flight>, free: Vec<usize>) -> Self {
        Self {
            world,
            flights,
            free,
            orders: Vec::new(),
        }
    }

    fn order(&mut self, robot: usize, action: Action, job: Job, engages: Vec<usize>) {
        self.free.retain(|&r| r != robot && !engages.contains(&r));
        self.orders.push(Order {
            robot,
            action,
            job,
            hold: Hold::None,
            engages,
        });
    }

    /// Whether a robot is already being helped, or engaged by a command.
    fn helped(&self, t: usize) -> bool {
        self.flights
            .values()
            .any(|f| f.job == Job::Rescue(t) || f.engages.contains(&t) || f.robot == t)
            || self
                .orders
                .iter()
                .any(|o| o.job == Job::Rescue(t) || o.engages.contains(&t) || o.robot == t)
    }

    /// The nearest free robot other than `t` with `cap` and the charge to help.
    fn helper(&self, t: usize, cap: CapabilityName) -> Option<usize> {
        let w = self.world;
        let at = w.pos(t);
        self.free
            .iter()
            .copied()
            .filter(|&h| h != t && w.can(h, cap) && w.battery(h) >= HELPER_FRAC * w.capacity_wh(h))
            .min_by(|&a, &b| {
                apart(w.pos(a), at)
                    .total_cmp(&apart(w.pos(b), at))
                    .then(a.cmp(&b))
            })
    }

    /// Whether any robot other than `t` that is not disabled can repair.
    fn repairer_exists(&self, t: usize) -> bool {
        let w = self.world;
        (0..w.robots.len()).any(|h| {
            h != t
                && w.can(h, CapabilityName::Repair)
                && w.status(h).is_some_and(|s| {
                    matches!(
                        s,
                        RobotStatus::Idle
                            | RobotStatus::Moving
                            | RobotStatus::Working
                            | RobotStatus::Charging
                    )
                })
        })
    }

    /// The next rescue step for every disabled robot in contact.
    pub fn rescue(&mut self, tried: &mut BTreeMap<usize, BTreeSet<usize>>) {
        let w = self.world;
        for t in 0..w.robots.len() {
            let Some(s) = w.state(t) else {
                continue;
            };
            if !s.in_relay_coverage || self.helped(t) {
                continue;
            }
            let pos = s.position;
            let id = w.robots[t].spec.robot_id.clone();
            match s.status {
                RobotStatus::Failed | RobotStatus::Stuck => {
                    if let Some(d) = w.depots.iter().find(|d| d.position == pos) {
                        let depot = d.depot_id.clone();
                        self.order(t, Action::Service { depot }, Job::Rescue(t), Vec::new());
                        continue;
                    }
                    let repairable = s.status == RobotStatus::Stuck
                        || s.faults
                            .iter()
                            .filter(|f| f.severity == Severity::Disabled)
                            .all(|f| f.field_repairable);
                    if repairable && self.repairer_exists(t) {
                        if let Some(h) = self.helper(t, CapabilityName::Repair) {
                            self.order(
                                h,
                                Action::Repair { target_robot: id },
                                Job::Rescue(t),
                                vec![t],
                            );
                        }
                        continue;
                    }
                    let to = w.nearest_depot(pos).map(|d| d.position).or_else(|| {
                        (s.status == RobotStatus::Stuck)
                            .then(|| w.nearest_station(pos).map(|st| st.position))
                            .flatten()
                    });
                    if let (Some(to), Some(h)) = (to, self.helper(t, CapabilityName::Haul)) {
                        self.order(
                            h,
                            Action::Tow {
                                target_robot: id,
                                to,
                            },
                            Job::Rescue(t),
                            vec![t],
                        );
                    }
                }
                RobotStatus::Offline => {
                    if let Some(st) = w.stations.iter().find(|st| st.position == pos) {
                        let source = st.station_id.clone();
                        self.order(
                            t,
                            Action::Charge {
                                source,
                                until_wh: None,
                            },
                            Job::Rescue(t),
                            Vec::new(),
                        );
                        continue;
                    }
                    let done = tried.entry(t).or_default();
                    let charger = self
                        .free
                        .iter()
                        .copied()
                        .filter(|&c| {
                            c != t && w.can(c, CapabilityName::Charge) && !done.contains(&c)
                        })
                        .min_by(|&a, &b| {
                            apart(w.pos(a), pos)
                                .total_cmp(&apart(w.pos(b), pos))
                                .then(a.cmp(&b))
                        });
                    if let Some(c) = charger {
                        if apart(w.pos(c), pos) <= REACH {
                            done.insert(c);
                            let source = w.robots[c].spec.robot_id.clone();
                            self.order(
                                t,
                                Action::Charge {
                                    source,
                                    until_wh: None,
                                },
                                Job::Rescue(t),
                                vec![c],
                            );
                        } else {
                            self.order(
                                c,
                                Action::MoveTo { target: pos },
                                Job::Rescue(t),
                                Vec::new(),
                            );
                        }
                        continue;
                    }
                    let to = w.nearest_station(pos).map(|st| st.position);
                    if let (Some(to), Some(h)) = (to, self.helper(t, CapabilityName::Haul)) {
                        self.order(
                            h,
                            Action::Tow {
                                target_robot: id,
                                to,
                            },
                            Job::Rescue(t),
                            vec![t],
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// Field repair for free robots with degraded faults, when a repairer is free.
    pub fn repair_degraded(&mut self) {
        let w = self.world;
        let faulty: Vec<usize> = self
            .free
            .iter()
            .copied()
            .filter(|&t| w.state(t).is_some_and(|s| !s.faults.is_empty()))
            .collect();
        for t in faulty {
            if !self.free.contains(&t) || self.helped(t) {
                continue;
            }
            if let Some(h) = self.helper(t, CapabilityName::Repair) {
                let id = w.robots[t].spec.robot_id.clone();
                self.order(
                    h,
                    Action::Repair { target_robot: id },
                    Job::Repair(t),
                    vec![t],
                );
            }
        }
    }

    /// Send free robots below the low-water mark to charge.
    pub fn charge_low(&mut self) {
        let low: Vec<usize> = self
            .free
            .iter()
            .copied()
            .filter(|&r| self.world.battery(r) < LOW_FRAC * self.world.capacity_wh(r))
            .collect();
        for r in low {
            self.charge(r);
        }
    }

    /// After allocation: a free robot that could do waiting work but cannot
    /// afford any of it charges.
    pub fn charge_unaffordable(&mut self, plan: &Plan, ready: &[u64]) {
        let w = self.world;
        let short: Vec<usize> = self
            .free
            .iter()
            .copied()
            .filter(|&r| w.battery(r) < 0.95 * w.capacity_wh(r))
            .filter(|&r| {
                let mut can = ready
                    .iter()
                    .filter_map(|&id| plan.estimate(id, r, w))
                    .peekable();
                can.peek().is_some() && can.all(|e| !w.affords(r, e.wh, e.end))
            })
            .collect();
        for r in short {
            self.charge(r);
        }
    }

    /// Charge `r` to full at the station with the shortest trip and queue.
    fn charge(&mut self, r: usize) {
        let w = self.world;
        let queued = |s: usize| {
            self.flights
                .values()
                .map(|f| &f.job)
                .chain(self.orders.iter().map(|o| &o.job))
                .filter(|j| **j == Job::Charge(s))
                .count()
        };
        let best = (0..w.stations.len()).min_by(|&a, &b| {
            let cost = |s: usize| {
                let st = &w.stations[s];
                let ahead = (queued(s) + 1).saturating_sub(st.slots.max(1) as usize);
                w.travel_s(r, w.pos(r), st.position) + ahead as f64 * QUEUE_S
            };
            cost(a).total_cmp(&cost(b)).then(a.cmp(&b))
        });
        if let Some(s) = best {
            let source = w.stations[s].station_id.clone();
            self.order(
                r,
                Action::Charge {
                    source,
                    until_wh: None,
                },
                Job::Charge(s),
                Vec::new(),
            );
        }
    }
}
