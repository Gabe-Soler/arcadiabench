//! Baseline coordinators: reference points for the leaderboard.
//!
//! Three baselines ship with the benchmark, as the binaries
//! `baseline-random`, `baseline-greedy` and `baseline-fcfs`. They share
//! everything but allocation:
//!
//! - **Task decomposition** ([`plan`]): the landing-pad goal as per-cell
//!   tasks (survey, dig, fill, grade, compact, sinter, verify) plus the berm.
//! - **The execution loop** (this module): take in each observation, keep
//!   track of every command in flight, and settle the plan as commands end.
//! - **Safety** ([`safety`]): rescue disabled robots (repair in the field,
//!   or tow to a depot and service, or a mobile charger for a flat
//!   battery), repair degraded faults, and charge when low.
//! - **Coverage:** every task stays in the base station's coverage, robots
//!   out of contact are never commanded, and a command refused
//!   `out_of_coverage` is retried after a pause.
//! - **Authority:** every level-2 command is reported with a `notify` in
//!   the decision that issues it, and nothing at level 3 or 4 is ever
//!   issued, so the baselines never wait on Earth and commit no violations.
//!
//! **Allocation** ([`alloc`]) is the only difference: random, greedy or
//! first come, first served.
//!
//! **Protocol only.** This module is an honest protocol client: it uses
//! `arcadia_protocol` and the standard library, and nothing else from
//! ArcadiaBench, so it knows only what any coordinator could know.
//! `tests/baselines.rs` checks this. It is deterministic: the same inputs
//! give the same decisions, with ordered collections, no clock, and a
//! generator seeded from the run's seed.

pub mod alloc;
pub mod plan;
pub mod safety;
pub mod world;

pub use alloc::Allocation;

use arcadia_protocol::*;
use plan::{End, Hold, Plan};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufReader};
use std::process::ExitCode;
use world::World;

/// The longest a baseline goes without a decision, s.
const WAKE_S: f64 = 600.0;
/// Pause for a robot after one of its commands is refused, s.
const BACKOFF_S: f64 = 60.0;

/// What a command in flight is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    /// A plan task.
    Task(u64),
    /// Charging at a station (by index).
    Charge(usize),
    /// A step in rescuing a disabled robot (by index).
    Rescue(usize),
    /// Repairing a robot's degraded faults (by index).
    Repair(usize),
}

/// A command in flight.
#[derive(Debug, Clone)]
pub struct Flight {
    pub robot: usize,
    pub job: Job,
    pub hold: Hold,
    /// Robots the command engages besides its own: repair and tow
    /// targets, a mobile charger.
    pub engages: Vec<usize>,
}

/// A command about to be issued.
#[derive(Debug, Clone)]
pub struct Order {
    pub robot: usize,
    pub action: Action,
    pub job: Job,
    pub hold: Hold,
    pub engages: Vec<usize>,
}

/// A baseline coordinator.
#[derive(Debug, Clone)]
pub struct Baseline {
    allocation: Allocation,
    world: Option<World>,
    plan: Option<Plan>,
    flights: BTreeMap<String, Flight>,
    issued: u64,
    rng: alloc::Rng,
    /// Mobile chargers already tried on each offline robot.
    tried: BTreeMap<usize, BTreeSet<usize>>,
}

impl Baseline {
    pub fn new(allocation: Allocation) -> Self {
        Self {
            allocation,
            world: None,
            plan: None,
            flights: BTreeMap::new(),
            issued: 0,
            rng: alloc::Rng::new(0),
            tried: BTreeMap::new(),
        }
    }

    /// The coordinator's name and version.
    pub fn info(&self) -> SoftwareInfo {
        SoftwareInfo {
            name: self.allocation.name().into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    /// Take in the scenario; returns the coordinator's name and version.
    pub fn welcome(&mut self, hello: &Hello) -> SoftwareInfo {
        self.world = Some(World::new(hello));
        self.rng = alloc::Rng::new(hello.seed);
        self.info()
    }

    /// The decision for an observation.
    pub fn decide(&mut self, obs: &Observation) -> Decision {
        let mut d = Decision::empty(obs.seq);
        let Some(world) = self.world.as_mut() else {
            return d;
        };
        world.observe(obs);
        for e in &obs.events {
            self.event(e, obs.t);
        }
        let t = obs.t;
        d.wake_at = Some(t + WAKE_S);
        let (Some(world), Some(plan)) = (self.world.as_mut(), self.plan.as_mut()) else {
            return d;
        };
        plan.review(t);

        // Free robots: idle, in contact, not engaged by a command in flight.
        let engaged: BTreeSet<usize> = self
            .flights
            .values()
            .flat_map(|f| f.engages.iter().copied().chain([f.robot]))
            .collect();
        let free: Vec<usize> = (0..world.robots.len())
            .filter(|&r| world.idle(r) && !engaged.contains(&r))
            .collect();
        for (r, robot) in world.robots.iter_mut().enumerate() {
            let is_free = free.contains(&r);
            if is_free && !robot.was_free {
                robot.free_since = t;
            }
            robot.was_free = is_free;
        }
        let world = &*world;
        self.tried.retain(|&r, _| {
            world.battery(r) < safety::LOW_FRAC * world.capacity_wh(r)
                || world.status(r) == Some(RobotStatus::Offline)
        });

        let mut crew = safety::Crew::new(world, &self.flights, free);
        crew.rescue(&mut self.tried);
        crew.repair_degraded();
        crew.charge_low();
        let ready = plan.ready(t);
        let assigned = alloc::allocate(
            self.allocation,
            world,
            plan,
            &ready,
            &mut crew.free,
            &mut self.rng,
        );
        crew.orders.extend(assigned.into_iter().map(|a| Order {
            robot: a.robot,
            action: a.action,
            job: Job::Task(a.task),
            hold: a.hold,
            engages: Vec::new(),
        }));
        let ready = plan.ready(t);
        crew.charge_unaffordable(plan, &ready);
        let orders = std::mem::take(&mut crew.orders);
        self.issue(orders, &mut d);

        // Wake for the first back-off to end, if sooner than the usual wake-up.
        if let Some(world) = &self.world {
            let soon = world
                .robots
                .iter()
                .map(|r| r.backoff_until)
                .filter(|&b| b > t)
                .fold(t + WAKE_S, f64::min);
            d.wake_at = Some(soon);
        }
        d
    }

    /// Issue orders as commands, with one `notify` per level-2 action.
    fn issue(&mut self, orders: Vec<Order>, d: &mut Decision) {
        let (Some(world), Some(plan)) = (self.world.as_ref(), self.plan.as_mut()) else {
            return;
        };
        let mut notify: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
        for o in orders {
            let name = o.action.name();
            let level = world.policy.level_for(name).get();
            if level >= 3 {
                // Never done autonomously: hand the task back.
                if let Job::Task(id) = o.job {
                    plan.finish(id, o.hold, End::Retry, world.t);
                }
                continue;
            }
            self.issued += 1;
            let command_id = format!("b-{:06}", self.issued);
            if level == 2 {
                notify.entry(name).or_default().push(command_id.clone());
            }
            d.commands.push(Command {
                command_id: command_id.clone(),
                robot_id: world.robots[o.robot].spec.robot_id.clone(),
                approval_id: None,
                action: o.action,
            });
            self.flights.insert(
                command_id,
                Flight {
                    robot: o.robot,
                    job: o.job,
                    hold: o.hold,
                    engages: o.engages,
                },
            );
        }
        for (action, commands) in notify {
            d.to_earth.push(SiteToEarth {
                message_id: format!("{}-notify", commands[0]),
                body: Downlink::Notify {
                    action: action.into(),
                    summary: format!("{} {action} command(s) issued", commands.len()),
                    commands: Some(commands),
                },
            });
        }
    }

    /// Take in one event.
    fn event(&mut self, e: &Event, now: f64) {
        let Some(world) = self.world.as_mut() else {
            return;
        };
        match &e.kind {
            EventKind::EarthMessage { message, .. } => match &message.body {
                Uplink::Goal {
                    objective: Objective::LandingPad { pad },
                    ..
                } if self.plan.is_none() => {
                    self.plan = Some(Plan::new(pad.clone(), world));
                }
                Uplink::PolicyUpdate { policy } => world.policy = policy.clone(),
                _ => {}
            },
            EventKind::ScanResult {
                command_id, cells, ..
            } => {
                world.scanned(cells);
                if let (Some(f), Some(plan)) = (self.flights.get(command_id), self.plan.as_mut())
                    && let (Job::Task(id), Hold::Scan { issued }) = (f.job, f.hold)
                {
                    plan.scanned(id, cells, issued, e.t, world);
                }
            }
            EventKind::CommandCompleted { command_id, .. } => {
                if let Some(f) = self.flights.remove(command_id)
                    && let (Job::Task(id), Some(plan)) = (f.job, self.plan.as_mut())
                {
                    plan.finish(id, f.hold, End::Completed, e.t);
                }
            }
            EventKind::CommandFailed {
                command_id, reason, ..
            } => {
                if let Some(f) = self.flights.remove(command_id) {
                    let end = match reason {
                        FailReason::Blocked => End::Wrong,
                        _ => End::Retry,
                    };
                    if let (Job::Task(id), Some(plan)) = (f.job, self.plan.as_mut()) {
                        plan.finish(id, f.hold, end, e.t);
                    }
                }
            }
            EventKind::CommandRejected {
                command_id, reason, ..
            } => {
                if let Some(f) = self.flights.remove(command_id) {
                    world.robots[f.robot].backoff_until = now + BACKOFF_S;
                    let end = match reason {
                        RejectReason::InvalidTarget => End::Wrong,
                        _ => End::Retry,
                    };
                    if let (Job::Task(id), Some(plan)) = (f.job, self.plan.as_mut()) {
                        if *reason == RejectReason::MissingCapability
                            && let Some(task) = plan.task(id)
                        {
                            world.robots[f.robot].lacks.insert(task.work.capability());
                        }
                        plan.finish(id, f.hold, end, e.t);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Run a baseline as a coordinator process: newline-delimited JSON on
/// stdin and stdout, as the protocol specifies.
pub fn serve(allocation: Allocation) -> ExitCode {
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = io::stdout().lock();
    let mut baseline = Baseline::new(allocation);
    let name = allocation.name();
    loop {
        let reply = match wire::recv::<_, SimToCoordinator>(&mut input) {
            Ok(Some(SimToCoordinator::Hello(h)))
                if is_compatible(&h.protocol_version, PROTOCOL_VERSION) =>
            {
                CoordinatorToSim::Welcome(Welcome {
                    protocol_version: PROTOCOL_VERSION.into(),
                    coordinator: baseline.welcome(&h),
                })
            }
            Ok(Some(SimToCoordinator::Hello(h))) => {
                let _ = wire::send(
                    &mut output,
                    &CoordinatorToSim::Error(ErrorMessage {
                        code: ErrorCode::VersionMismatch,
                        message: format!("got {}, want {PROTOCOL_VERSION}", h.protocol_version),
                    }),
                );
                return ExitCode::FAILURE;
            }
            Ok(Some(SimToCoordinator::Observation(obs))) => {
                CoordinatorToSim::Decision(baseline.decide(&obs))
            }
            Ok(Some(SimToCoordinator::End(_))) | Ok(None) => return ExitCode::SUCCESS,
            Ok(Some(SimToCoordinator::Error(e))) => {
                eprintln!("{name}: simulator error: {}", e.message);
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("{name}: {e}");
                return ExitCode::FAILURE;
            }
        };
        if wire::send(&mut output, &reply).is_err() {
            return ExitCode::FAILURE;
        }
    }
}
