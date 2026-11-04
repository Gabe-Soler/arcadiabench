//! The discrete-event simulator.
//!
//! Robot actions, environment changes, failures and power accounting are
//! events on one deterministic queue. [`Sim::advance`] runs events until the
//! coordinator has something to decide, then returns an observation;
//! [`Sim::apply`] feeds the decision back in. Command execution lives in
//! [`actions`] and [`repair`], failures in [`hazards`] and [`faults`], and the
//! run metrics in [`metrics`].

pub mod actions;
pub mod environment;
pub mod faults;
pub mod hazards;
pub mod metrics;
pub mod mission;
pub mod params;
pub mod power;
pub mod queue;
pub mod repair;
pub mod rng;
pub mod robot;
pub mod terrain;
pub mod time;

use crate::scenario::Generated;
use arcadia_protocol::*;
use environment::{EnvEvent, Environment};
use faults::Exposure;
use hazards::HazardKind;
use metrics::Tally;
use mission::Mission;
use params::Params;
use power::Grid;
use queue::Queue;
use rng::Rng;
use robot::{RobotRt, Step};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use terrain::Terrain;
use time::{SimMs, ms, secs};

#[derive(Debug, Clone, Copy)]
pub(crate) enum Ev {
    StepDone {
        robot: usize,
        generation: u64,
    },
    SessionDone {
        robot: usize,
        generation: u64,
    },
    Depleted {
        robot: usize,
        generation: u64,
    },
    /// A failure process fires (stale if the robot's exposure changed since).
    Hazard {
        robot: usize,
        generation: u64,
        kind: HazardKind,
    },
    /// The scripted incident at this index of `params.scripted` happens.
    Scripted(usize),
    PowerTick,
    Env(EnvEvent),
    Wake {
        generation: u64,
    },
    TimeLimit,
}

pub enum Advance {
    Observe(Observation),
    End { t: SimTime, reason: EndReason },
}

#[derive(Debug, Clone)]
pub(crate) struct StationRt {
    pub spec: ChargeStation,
    pub occupants: Vec<usize>,
    pub waiting: VecDeque<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct DepotRt {
    pub spec: Depot,
    /// Robots being serviced.
    pub occupants: Vec<usize>,
    /// Robots waiting for a service bay, first come first served.
    pub waiting: VecDeque<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Source {
    Station(usize),
    Mobile(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct Session {
    pub robot: usize,
    pub source: Source,
    pub power_w: f64,
    pub target_battery_wh: f64,
    /// After the battery is full, keep going to refill the charge tank.
    pub fill_tank: bool,
}

/// Robots reach each other (to charge, repair, tow or strip) within this many cells.
pub(crate) const REACH_CELLS: f64 = 1.5;

/// Telemetry noise drawn for one robot in one observation.
struct Noise {
    battery_wh: f64,
    health: [f64; 4],
}

pub struct Sim {
    pub(crate) params: Params,
    pub(crate) now: SimMs,
    pub(crate) start: SimMs,
    pub(crate) time_limit: SimMs,
    pub(crate) queue: Queue<Ev>,
    pub(crate) terrain: Terrain,
    pub(crate) robots: Vec<RobotRt>,
    pub(crate) robot_ix: BTreeMap<String, usize>,
    pub(crate) stations: Vec<StationRt>,
    pub(crate) station_ix: BTreeMap<String, usize>,
    pub(crate) depots: Vec<DepotRt>,
    pub(crate) depot_ix: BTreeMap<String, usize>,
    pub(crate) sessions: Vec<Session>,
    pub(crate) grid: Grid,
    pub(crate) env: Environment,
    pub(crate) mission: Option<Mission>,
    pub(crate) policy: Policy,
    pub(crate) pending: Vec<Event>,
    pub(crate) decision_due: bool,
    pub(crate) wake_generation: u64,
    pub(crate) seq: u64,
    pub(crate) last_power_ms: SimMs,
    pub(crate) last_fraction: f64,
    pub(crate) rng_work: Rng,
    pub(crate) rng_scan: Rng,
    pub(crate) rng_telemetry: Rng,
    pub(crate) rng_health: Rng,
    pub(crate) rng_wear_faults: Rng,
    pub(crate) rng_random_faults: Rng,
    pub(crate) rng_storm_faults: Rng,
    pub(crate) rng_stuck: Rng,
    /// Duration noise of repair, tow, strip and install work.
    pub(crate) rng_maintenance: Rng,
    pub(crate) changed: BTreeSet<(u32, u32)>,
    pub(crate) command_ids: BTreeSet<String>,
    pub(crate) mission_dirty: bool,
    pub(crate) ended: Option<EndReason>,
    pub(crate) tally: Tally,
    same_time: (SimMs, u64),
}

impl Sim {
    pub fn new(generated: Generated, seed: u64, params: Params) -> Self {
        let Generated {
            scenario,
            terrain,
            goal,
            mission,
            starts,
        } = generated;
        let time_limit = ms(scenario.time_limit_t);
        let env = Environment::generate(seed, time_limit, &params);
        let robots: Vec<RobotRt> = scenario
            .robots
            .iter()
            .zip(starts)
            .map(|(spec, pos)| RobotRt::new(spec.clone(), pos))
            .collect();
        let robot_ix = robots
            .iter()
            .enumerate()
            .map(|(i, r)| (r.spec.robot_id.clone(), i))
            .collect();
        let stations: Vec<StationRt> = scenario
            .stations
            .iter()
            .map(|s| StationRt {
                spec: s.clone(),
                occupants: Vec::new(),
                waiting: VecDeque::new(),
            })
            .collect();
        let station_ix = stations
            .iter()
            .enumerate()
            .map(|(i, s)| (s.spec.station_id.clone(), i))
            .collect();
        let depots: Vec<DepotRt> = scenario
            .depots
            .iter()
            .map(|d| DepotRt {
                spec: d.clone(),
                occupants: Vec::new(),
                waiting: VecDeque::new(),
            })
            .collect();
        let depot_ix = depots
            .iter()
            .enumerate()
            .map(|(i, d)| (d.spec.depot_id.clone(), i))
            .collect();
        let grid = Grid::new(&params);
        let tally = Tally::new(robots.len(), grid.bank_wh);
        let start = ms(scenario.start_t);

        let mut sim = Self {
            grid,
            now: start,
            start,
            time_limit,
            queue: Queue::default(),
            terrain,
            robots,
            robot_ix,
            stations,
            station_ix,
            depots,
            depot_ix,
            sessions: Vec::new(),
            env,
            mission: Some(mission),
            policy: scenario.policy.clone(),
            pending: Vec::new(),
            decision_due: true,
            wake_generation: 0,
            seq: 0,
            last_power_ms: start,
            last_fraction: 1.0,
            rng_work: Rng::stream(seed, "work"),
            rng_scan: Rng::stream(seed, "scan"),
            rng_telemetry: Rng::stream(seed, "telemetry"),
            rng_health: Rng::stream(seed, "health"),
            rng_wear_faults: Rng::stream(seed, "wear_faults"),
            rng_random_faults: Rng::stream(seed, "random_faults"),
            rng_storm_faults: Rng::stream(seed, "storm_faults"),
            rng_stuck: Rng::stream(seed, "stuck"),
            rng_maintenance: Rng::stream(seed, "maintenance"),
            changed: BTreeSet::new(),
            command_ids: BTreeSet::new(),
            mission_dirty: false,
            ended: None,
            tally,
            same_time: (u64::MAX, 0),
            params,
        };

        for (t, e) in sim.env.schedule(time_limit) {
            sim.queue.push(t, Ev::Env(e));
        }
        sim.queue.push(time_limit, Ev::TimeLimit);
        sim.queue
            .push(sim.now + ms(sim.params.power_tick_s), Ev::PowerTick);
        for (i, s) in sim.params.scripted.iter().enumerate() {
            sim.queue.push(ms(s.t_s).max(sim.now), Ev::Scripted(i));
        }
        for r in 0..sim.robots.len() {
            sim.schedule_depletion(r);
        }

        // What the coordinator sees at t = start: the link, the sky, the grid, and its goal.
        sim.emit(
            EventKind::Comms {
                link: Link::Up,
                cause: CommsCause::Nominal,
                one_way_delay_s: sim.params.one_way_delay_s,
                expected_restore_t: None,
            },
            true,
        );
        sim.emit(sim.weather_event(), true);
        sim.emit(sim.power_event(), true);
        let now = secs(sim.now);
        sim.emit(
            EventKind::EarthMessage {
                sent_t: now,
                message: goal,
            },
            true,
        );
        sim
    }

    pub fn now(&self) -> SimTime {
        secs(self.now)
    }

    /// Run until the coordinator has something to decide, or the run ends.
    pub fn advance(&mut self) -> Advance {
        loop {
            if let Some(reason) = self.end_check() {
                return Advance::End {
                    t: secs(self.now),
                    reason,
                };
            }
            if self.decision_due {
                self.decision_due = false;
                if self.same_time.0 == self.now {
                    self.same_time.1 += 1;
                } else {
                    self.same_time = (self.now, 1);
                }
                if self.same_time.1 > self.params.max_observations_at_same_time {
                    self.ended = Some(EndReason::CoordinatorError);
                    continue;
                }
                return Advance::Observe(self.observation());
            }
            let (t, ev) = self
                .queue
                .pop()
                .expect("the time-limit event is always queued");
            self.now = t;
            self.handle(ev);
            self.refresh_out_of_action();
        }
    }

    /// Apply a coordinator decision. Returns the Earth-bound messages with
    /// their delivery times.
    pub fn apply(&mut self, decision: &Decision) -> Vec<(SiteToEarth, SimTime)> {
        self.tally.decisions += 1;
        for command in &decision.commands {
            self.tally.commands += 1;
            self.submit(command);
        }
        self.wake_generation += 1;
        if let Some(w) = decision.wake_at {
            let at = ms(w);
            if at > self.now {
                self.queue.push(
                    at,
                    Ev::Wake {
                        generation: self.wake_generation,
                    },
                );
            }
        }
        for m in &decision.to_earth {
            self.tally.downlink(self.now, m, &self.policy);
        }
        self.refresh_out_of_action();
        let deliver = secs(self.now) + self.params.one_way_delay_s;
        decision
            .to_earth
            .iter()
            .map(|m| (m.clone(), deliver))
            .collect()
    }

    /// Ground-truth robot states, free of telemetry noise.
    pub fn truth_robots(&self) -> Vec<RobotState> {
        (0..self.robots.len())
            .map(|r| self.robot_state(r, None))
            .collect()
    }

    /// Cells changed since the last call, as ground truth.
    pub fn take_changed_cells(&mut self) -> Vec<CellObservation> {
        std::mem::take(&mut self.changed)
            .into_iter()
            .map(|(x, y)| self.terrain.truth_observation(GridCoord { x, y }))
            .collect()
    }

    /// Events that happened but were never delivered in an observation
    /// (those of the run's final instant).
    pub fn take_pending(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    /// True state of the site grid, sky and Earth link now.
    pub fn site_truth(&self) -> SiteTruth {
        let requested = self.station_requested_w();
        let (storage_wh, fraction) = if self.now > self.last_power_ms {
            let (bank, s) = self
                .grid
                .preview(&self.env, self.last_power_ms, self.now, requested);
            (bank, s.fraction)
        } else {
            (self.grid.bank_wh, self.last_fraction)
        };
        SiteTruth {
            storage_wh,
            solar_w: self.grid.solar_w(&self.env, self.now),
            fission_w: self.grid.fission_w,
            demand_w: self.grid.essential_load_w + requested * fraction,
            optical_depth: self.env.tau(self.now),
            temperature_c: Environment::temperature_c(self.now),
            link: Link::Up,
            one_way_delay_s: self.params.one_way_delay_s,
        }
    }

    /// Fraction of the pad meeting spec.
    pub fn progress(&self) -> f64 {
        self.mission
            .as_ref()
            .map_or(0.0, |m| m.progress(&self.terrain))
    }

    // ----- engine internals -----

    fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::StepDone { robot, generation } => {
                if self.robots[robot].generation == generation {
                    self.finish_step(robot);
                }
            }
            Ev::SessionDone { robot, generation } => {
                if self.robots[robot].generation == generation {
                    self.complete_session(robot);
                }
            }
            Ev::Depleted { robot, generation } => {
                if self.robots[robot].generation == generation {
                    self.deplete(robot);
                }
            }
            Ev::Hazard {
                robot,
                generation,
                kind,
            } => {
                if self.robots[robot].hazard_generation == generation {
                    self.on_hazard(robot, kind);
                }
            }
            Ev::Scripted(i) => self.on_scripted(i),
            Ev::PowerTick => {
                self.settle_power();
                self.repredict_sessions();
                self.check_bank_alert();
                self.queue
                    .push(self.now + ms(self.params.power_tick_s), Ev::PowerTick);
            }
            Ev::Env(e) => {
                self.settle_power();
                self.repredict_sessions();
                // Weather is reported when a storm starts, peaks and ends, and at
                // every sunrise while one is raging, so its course is visible.
                let report_weather = match e {
                    EnvEvent::Sunrise => self.env.storm_at(self.now).is_some(),
                    EnvEvent::Noon | EnvEvent::Sunset => false,
                    EnvEvent::StormStart(_) | EnvEvent::StormPeak(_) | EnvEvent::StormEnd(_) => {
                        true
                    }
                };
                if report_weather {
                    self.emit(self.weather_event(), true);
                }
                self.emit(self.power_event(), true);
            }
            Ev::Wake { generation } => {
                if generation == self.wake_generation {
                    self.decision_due = true;
                }
            }
            Ev::TimeLimit => self.ended = Some(EndReason::TimeLimit),
        }
    }

    fn end_check(&mut self) -> Option<EndReason> {
        if self.ended.is_none()
            && self.mission_dirty
            && let Some(m) = &self.mission
        {
            self.mission_dirty = false;
            if m.complete(&self.terrain) {
                self.ended = Some(EndReason::GoalsComplete);
            }
        }
        if self.ended.is_none() && self.fleet_lost() {
            self.ended = Some(EndReason::FleetLost);
        }
        self.ended
    }

    /// No robot can ever act again (semantics §6.8).
    fn fleet_lost(&self) -> bool {
        !(0..self.robots.len()).any(|r| {
            let robot = &self.robots[r];
            robot.commandable() || robot.in_service() || self.recoverable_in_place(r)
        })
    }

    /// Whether a disabled robot could become commandable, without moving, by
    /// the commands it may still accept: service on a depot's cell, then
    /// charge from a station on its cell.
    fn recoverable_in_place(&self, r: usize) -> bool {
        let robot = &self.robots[r];
        if robot.lost || robot.towed {
            return false;
        }
        let on_depot = self.depots.iter().any(|d| d.spec.position == robot.pos);
        let on_station = self.stations.iter().any(|s| s.spec.position == robot.pos);
        (!robot.health.has_disabled() || on_depot)
            && (!robot.health.stuck || on_depot)
            && (!robot.offline || on_station)
    }

    fn observation(&mut self) -> Observation {
        let seq = self.seq;
        self.seq += 1;
        let robots = (0..self.robots.len())
            .map(|r| {
                let noise = Noise {
                    battery_wh: self.params.telemetry_battery_noise_frac
                        * self.robots[r].spec.battery_capacity_wh
                        * self.rng_telemetry.normal(),
                    health: std::array::from_fn(|_| self.rng_health.normal()),
                };
                self.robot_state(r, Some(&noise))
            })
            .collect();
        Observation {
            seq,
            t: secs(self.now),
            events: std::mem::take(&mut self.pending),
            robots,
        }
    }

    fn robot_state(&self, r: usize, noise: Option<&Noise>) -> RobotState {
        let robot = &self.robots[r];
        if let Some(frozen) = &robot.frozen {
            return RobotState {
                telemetry_t: secs(self.now),
                ..frozen.clone()
            };
        }
        let p = &self.params;
        let mut battery = robot.battery_wh - self.pending_drain_wh(r);
        let heater_on = self.env.is_cold(self.now) && !robot.offline;
        let mut health = robot
            .health
            .telemetry(p, Environment::temperature_c(self.now), heater_on);
        if let Some(n) = noise {
            battery += n.battery_wh;
            health.wheel_slip =
                (health.wheel_slip + n.health[0] * p.telemetry_slip_noise).clamp(0.0, 1.0);
            health.motor_current_ratio =
                (health.motor_current_ratio + n.health[1] * p.telemetry_current_noise).max(0.0);
            health.dust_coverage =
                (health.dust_coverage + n.health[2] * p.telemetry_dust_noise).clamp(0.0, 1.0);
            health.temperature_c += n.health[3] * p.telemetry_temp_noise_c;
        }
        RobotState {
            robot_id: robot.spec.robot_id.clone(),
            position: robot.position_at(self.now),
            status: robot.reported_status(),
            battery_wh: battery.clamp(0.0, robot.spec.battery_capacity_wh),
            payload_kg: robot.payload_kg,
            current_command: robot.running_command().map(String::from),
            health,
            faults: robot.health.active_faults(),
            in_relay_coverage: true,
            telemetry_t: secs(self.now),
        }
    }

    pub(crate) fn emit(&mut self, kind: EventKind, decision: bool) {
        self.pending.push(Event {
            t: secs(self.now),
            kind,
        });
        if decision {
            self.decision_due = true;
        }
    }

    fn weather_event(&self) -> EventKind {
        let condition = if self.env.storm_at(self.now).is_some() {
            WeatherCondition::DustStorm
        } else {
            WeatherCondition::Clear
        };
        EventKind::Weather {
            condition,
            optical_depth: self.env.tau(self.now),
            temperature_c: Environment::temperature_c(self.now),
        }
    }

    /// Charging power requested by robots at stations, W.
    fn station_requested_w(&self) -> f64 {
        self.sessions
            .iter()
            .filter(|s| matches!(s.source, Source::Station(_)))
            .map(|s| s.power_w)
            .sum()
    }

    fn power_event(&self) -> EventKind {
        EventKind::Power {
            solar_w: self.grid.solar_w(&self.env, self.now),
            fission_w: self.grid.fission_w,
            demand_w: self.grid.essential_load_w + self.station_requested_w() * self.last_fraction,
            storage_wh: self.grid.bank_wh,
            storage_capacity_wh: self.grid.bank_capacity_wh,
            daylight: Environment::daylight(self.now),
        }
    }

    fn check_bank_alert(&mut self) {
        if self.grid.alert().is_some() {
            self.emit(self.power_event(), true);
        }
    }

    /// Replace the robot's scheduled event.
    pub(crate) fn schedule(&mut self, r: usize, at: SimMs, make: fn(usize, u64) -> Ev) {
        let robot = &mut self.robots[r];
        robot.generation += 1;
        self.queue.push(at, make(r, robot.generation));
    }

    /// Cancel whatever the robot had scheduled.
    pub(crate) fn unschedule(&mut self, r: usize) {
        self.robots[r].generation += 1;
    }

    // ----- a robot's own energy -----

    /// Idle drain (plus the heater while it is cold) over `[t0, t1)`, Wh.
    fn drain_wh(&self, r: usize, t0: SimMs, t1: SimMs) -> f64 {
        if t1 <= t0 {
            return 0.0;
        }
        let robot = &self.robots[r];
        let idle = robot.spec.idle_power_w;
        let heater = self.params.heater_idle_factor * idle;
        let cold_h = secs(self.env.cold_ms(t0, t1)) / 3600.0;
        (idle * secs(t1 - t0) / 3600.0 + heater * cold_h) * robot.health.energy_factor(&self.params)
    }

    /// Drain owed since the robot last settled, Wh.
    fn pending_drain_wh(&self, r: usize) -> f64 {
        self.robots[r]
            .drain_since
            .map_or(0.0, |since| self.drain_wh(r, since, self.now))
    }

    /// Apply idle drain up to now.
    pub(crate) fn settle_drain(&mut self, r: usize) {
        let Some(since) = self.robots[r].drain_since else {
            return;
        };
        let used = self.drain_wh(r, since, self.now);
        let robot = &mut self.robots[r];
        let drawn = used.min(robot.battery_wh.max(0.0));
        robot.battery_wh = (robot.battery_wh - used).max(0.0);
        robot.drain_since = Some(self.now);
        robot.health.grow(
            &self.params,
            &Exposure {
                kwh: drawn / 1000.0,
                ..Exposure::default()
            },
        );
    }

    /// When idle drain empties the battery, if it does before the time limit.
    fn time_to_empty(&self, r: usize) -> Option<SimMs> {
        let robot = &self.robots[r];
        robot.drain_since?;
        let factor = robot.health.energy_factor(&self.params);
        let idle_w = robot.spec.idle_power_w * factor;
        let heater_w = self.params.heater_idle_factor * robot.spec.idle_power_w * factor;
        if idle_w <= 0.0 && heater_w <= 0.0 {
            return None;
        }
        let mut left = robot.battery_wh - self.pending_drain_wh(r);
        let mut t = self.now;
        loop {
            if left <= 0.0 {
                return Some(t);
            }
            let rate_w = idle_w + if self.env.is_cold(t) { heater_w } else { 0.0 };
            let next = self.env.next_cold_change(t).unwrap_or(u64::MAX);
            if rate_w > 0.0 {
                let empty = t.saturating_add(ms(left / rate_w * 3600.0));
                if empty <= next {
                    return (empty <= self.time_limit).then_some(empty);
                }
                left -= rate_w * secs(next - t) / 3600.0;
            }
            if next > self.time_limit {
                return None;
            }
            t = next;
        }
    }

    pub(crate) fn schedule_depletion(&mut self, r: usize) {
        match self.time_to_empty(r) {
            Some(at) => self.schedule(r, at, |robot, generation| Ev::Depleted {
                robot,
                generation,
            }),
            None => self.unschedule(r),
        }
    }

    // ----- power and charging sessions -----

    /// Settle grid and every charging session up to now.
    pub(crate) fn settle_power(&mut self) {
        let (t0, t1) = (self.last_power_ms, self.now);
        if t1 > t0 {
            let s = self
                .grid
                .settle(&self.env, t0, t1, self.station_requested_w());
            if s.deficit {
                self.tally.deficit_ms += t1 - t0;
            }
            let dt_h = secs(t1 - t0) / 3600.0;
            for i in 0..self.sessions.len() {
                let session = self.sessions[i].clone();
                let offered = match session.source {
                    Source::Station(_) => session.power_w * s.fraction * dt_h,
                    Source::Mobile(c) => (session.power_w * dt_h).min(self.robots[c].tank_wh),
                };
                let given = self.give_energy(&session, offered);
                match session.source {
                    Source::Station(_) => self.tally.grid_energy_wh += given,
                    Source::Mobile(c) => self.robots[c].tank_wh -= given,
                }
            }
            self.last_fraction = s.fraction;
            self.tally.min_storage_wh = self.tally.min_storage_wh.min(self.grid.bank_wh);
        }
        self.last_power_ms = t1;
    }

    /// Energy the session still needs.
    fn session_need(&self, s: &Session) -> f64 {
        let r = &self.robots[s.robot];
        let mut need = (s.target_battery_wh - r.battery_wh).max(0.0);
        if s.fill_tank {
            need += (r.tank_capacity_wh() - r.tank_wh).max(0.0);
        }
        need
    }

    /// Deliver up to `offered` Wh: battery first, then tank. Returns the amount used.
    fn give_energy(&mut self, s: &Session, offered: f64) -> f64 {
        let r = &mut self.robots[s.robot];
        let to_battery = offered.min((s.target_battery_wh - r.battery_wh).max(0.0));
        r.battery_wh += to_battery;
        r.received_wh += to_battery;
        if to_battery > 0.0 {
            r.offline = false;
        }
        let mut used = to_battery;
        if s.fill_tank {
            let to_tank = (offered - to_battery).min((r.tank_capacity_wh() - r.tank_wh).max(0.0));
            r.tank_wh += to_tank;
            used += to_tank;
        }
        used
    }

    /// Re-predict when every session finishes, at the current delivery rate.
    pub(crate) fn repredict_sessions(&mut self) {
        for i in 0..self.sessions.len() {
            let s = self.sessions[i].clone();
            let need = self.session_need(&s);
            let (rate_w, cap) = match s.source {
                Source::Station(_) => (s.power_w * self.last_fraction, need),
                Source::Mobile(c) => (s.power_w, need.min(self.robots[c].tank_wh)),
            };
            let done = |robot, generation| Ev::SessionDone { robot, generation };
            if cap <= 1e-6 {
                self.schedule(s.robot, self.now, done);
            } else if rate_w > 0.0 {
                self.schedule(s.robot, self.now + ms(cap / rate_w * 3600.0).max(1), done);
            } else {
                self.unschedule(s.robot);
            }
        }
    }

    /// A robot starts drawing from a session: idle drain stops and, once
    /// energy flows, it is no longer offline.
    fn begin_session(&mut self, r: usize, rate_w: f64) {
        self.settle_drain(r);
        let robot = &mut self.robots[r];
        robot.drain_since = None;
        robot.status = RobotStatus::Charging;
        if rate_w > 0.0 {
            robot.offline = false;
        }
    }

    pub(crate) fn start_station_session(
        &mut self,
        r: usize,
        station: usize,
        until_wh: Option<f64>,
    ) {
        self.settle_power();
        let st = &mut self.stations[station];
        st.occupants.push(r);
        let power_w = st.spec.max_power_w / f64::from(st.spec.slots.max(1));
        self.begin_session(r, power_w * self.last_fraction);
        let robot = &self.robots[r];
        let cap = robot.spec.battery_capacity_wh;
        let fill_tank = until_wh.is_none() && robot.tank_capacity_wh() > 0.0;
        self.sessions.push(Session {
            robot: r,
            source: Source::Station(station),
            power_w,
            target_battery_wh: until_wh.unwrap_or(cap).min(cap),
            fill_tank,
        });
        self.repredict_sessions();
    }

    pub(crate) fn start_mobile_session(&mut self, r: usize, charger: usize, until_wh: Option<f64>) {
        self.settle_power();
        let rate_w = self.robots[charger]
            .capability(CapabilityName::Charge)
            .map_or(0.0, |c| c.rate * 3600.0);
        self.robots[charger].engaged_by = Some(r);
        let flowing = if self.robots[charger].tank_wh > 0.0 {
            rate_w
        } else {
            0.0
        };
        self.begin_session(r, flowing);
        let cap = self.robots[r].spec.battery_capacity_wh;
        self.sessions.push(Session {
            robot: r,
            source: Source::Mobile(charger),
            power_w: rate_w,
            target_battery_wh: until_wh.unwrap_or(cap).min(cap),
            fill_tank: false,
        });
        self.repredict_sessions();
    }

    /// End `r`'s session (or queue slot) without finishing its step.
    pub(crate) fn leave_charging(&mut self, r: usize) {
        self.settle_power();
        if let Some(i) = self.sessions.iter().position(|s| s.robot == r) {
            let s = self.sessions.remove(i);
            match s.source {
                Source::Station(st) => {
                    self.stations[st].occupants.retain(|&o| o != r);
                    self.admit_next(st);
                }
                Source::Mobile(c) => self.robots[c].engaged_by = None,
            }
        }
        for st in &mut self.stations {
            st.waiting.retain(|&w| w != r);
        }
        self.repredict_sessions();
    }

    /// Give a freed station slot to the next robot in line.
    fn admit_next(&mut self, st: usize) {
        if let Some(next) = self.stations[st].waiting.pop_front() {
            let until = match self.robots[next]
                .activity
                .as_ref()
                .and_then(|a| a.running.as_ref())
            {
                Some(robot::Running {
                    step: Step::ChargeStation { until_wh, .. },
                    ..
                }) => *until_wh,
                _ => None,
            };
            self.start_station_session(next, st, until);
        }
    }

    fn complete_session(&mut self, r: usize) {
        self.settle_power();
        if let Some(i) = self.sessions.iter().position(|s| s.robot == r) {
            let s = self.sessions[i].clone();
            // Snap to exactly full when the prediction landed within rounding.
            if self.session_need(&s) < 0.01 {
                let robot = &mut self.robots[r];
                let topped = (s.target_battery_wh - robot.battery_wh).max(0.0);
                robot.battery_wh += topped;
                robot.received_wh += topped;
                if s.fill_tank {
                    robot.tank_wh = robot.tank_capacity_wh();
                }
            }
        }
        self.leave_charging(r);
        if let Some(a) = self.robots[r].activity.as_mut() {
            a.running = None;
        }
        self.start_next_step(r);
    }

    fn deplete(&mut self, r: usize) {
        self.settle_drain(r);
        self.robots[r].battery_wh = 0.0;
        // A dead mobile charger ends the sessions it was feeding.
        let fed: Vec<usize> = self
            .sessions
            .iter()
            .filter(|s| s.source == Source::Mobile(r))
            .map(|s| s.robot)
            .collect();
        for needy in fed {
            self.complete_session(needy);
        }
        if self.robots[r].activity.is_some() {
            self.fail_command(
                r,
                FailReason::InsufficientEnergy,
                Some("battery depleted".into()),
            );
        } else {
            let robot = &mut self.robots[r];
            robot.offline = true;
            robot.drain_since = None;
            self.unschedule(r);
            self.decision_due = true;
        }
    }
}
