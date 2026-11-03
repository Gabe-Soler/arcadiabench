//! The discrete-event simulator.
//!
//! Robot actions, environment changes and power accounting are events on one
//! deterministic queue. [`Sim::advance`] runs events until the coordinator has
//! something to decide, then returns an observation; [`Sim::apply`] feeds the
//! decision back in. Command execution lives in [`actions`].

pub mod actions;
pub mod environment;
pub mod mission;
pub mod params;
pub mod power;
pub mod queue;
pub mod rng;
pub mod robot;
pub mod terrain;
pub mod time;

use crate::scenario::Generated;
use arcadia_protocol::*;
use environment::{EnvEvent, Environment};
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
    StepDone { robot: usize, generation: u64 },
    SessionDone { robot: usize, generation: u64 },
    Depleted { robot: usize, generation: u64 },
    PowerTick,
    Env(EnvEvent),
    Wake { generation: u64 },
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

pub struct Sim {
    pub(crate) params: Params,
    pub(crate) now: SimMs,
    pub(crate) queue: Queue<Ev>,
    pub(crate) terrain: Terrain,
    pub(crate) robots: Vec<RobotRt>,
    pub(crate) robot_ix: BTreeMap<String, usize>,
    pub(crate) stations: Vec<StationRt>,
    pub(crate) station_ix: BTreeMap<String, usize>,
    pub(crate) sessions: Vec<Session>,
    pub(crate) grid: Grid,
    pub(crate) env: Environment,
    pub(crate) mission: Option<Mission>,
    pub(crate) pending: Vec<Event>,
    pub(crate) decision_due: bool,
    pub(crate) wake_generation: u64,
    pub(crate) seq: u64,
    pub(crate) last_power_ms: SimMs,
    pub(crate) last_fraction: f64,
    pub(crate) rng_work: Rng,
    pub(crate) rng_scan: Rng,
    pub(crate) rng_telemetry: Rng,
    pub(crate) changed: BTreeSet<(u32, u32)>,
    pub(crate) command_ids: BTreeSet<String>,
    pub(crate) mission_dirty: bool,
    pub(crate) ended: Option<EndReason>,
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

        let mut sim = Self {
            grid: Grid::new(&params),
            params,
            now: ms(scenario.start_t),
            queue: Queue::default(),
            terrain,
            robots,
            robot_ix,
            stations,
            station_ix,
            sessions: Vec::new(),
            env,
            mission: Some(mission),
            pending: Vec::new(),
            decision_due: true,
            wake_generation: 0,
            seq: 0,
            last_power_ms: ms(scenario.start_t),
            last_fraction: 1.0,
            rng_work: Rng::stream(seed, "work"),
            rng_scan: Rng::stream(seed, "scan"),
            rng_telemetry: Rng::stream(seed, "telemetry"),
            changed: BTreeSet::new(),
            command_ids: BTreeSet::new(),
            mission_dirty: false,
            ended: None,
            same_time: (u64::MAX, 0),
        };

        for (t, e) in sim.env.schedule(time_limit) {
            sim.queue.push(t, Ev::Env(e));
        }
        sim.queue.push(time_limit, Ev::TimeLimit);
        sim.queue
            .push(sim.now + ms(sim.params.power_tick_s), Ev::PowerTick);
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
        }
    }

    /// Apply a coordinator decision. Returns the Earth-bound messages with
    /// their delivery times.
    pub fn apply(&mut self, decision: &Decision) -> Vec<(SiteToEarth, SimTime)> {
        for command in &decision.commands {
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
        self.ended
    }

    fn observation(&mut self) -> Observation {
        let seq = self.seq;
        self.seq += 1;
        let robots = (0..self.robots.len())
            .map(|r| {
                let noise = self.params.telemetry_battery_noise_frac
                    * self.robots[r].spec.battery_capacity_wh
                    * self.rng_telemetry.normal();
                self.robot_state(r, Some(noise))
            })
            .collect();
        Observation {
            seq,
            t: secs(self.now),
            events: std::mem::take(&mut self.pending),
            robots,
        }
    }

    fn robot_state(&self, r: usize, battery_noise: Option<f64>) -> RobotState {
        let robot = &self.robots[r];
        let mut battery = robot.battery_wh;
        if let Some(since) = robot.drain_since {
            battery -= robot.spec.idle_power_w * secs(self.now.saturating_sub(since)) / 3600.0;
        }
        if let Some(n) = battery_noise {
            battery += n;
        }
        RobotState {
            robot_id: robot.spec.robot_id.clone(),
            position: robot.position_at(self.now),
            status: robot.status,
            battery_wh: battery.clamp(0.0, robot.spec.battery_capacity_wh),
            payload_kg: robot.payload_kg,
            current_command: robot.running_command().map(String::from),
            health: HealthTelemetry {
                wheel_slip: 0.02,
                motor_current_ratio: 1.0,
                dust_coverage: 0.0,
                temperature_c: Environment::temperature_c(self.now),
            },
            in_relay_coverage: true,
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

    fn power_event(&self) -> EventKind {
        let charging_w: f64 = self
            .sessions
            .iter()
            .filter(|s| matches!(s.source, Source::Station(_)))
            .map(|s| s.power_w * self.last_fraction)
            .sum();
        EventKind::Power {
            solar_w: self.grid.solar_w(&self.env, self.now),
            fission_w: self.grid.fission_w,
            demand_w: self.grid.base_load_w + charging_w,
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

    pub(crate) fn schedule_depletion(&mut self, r: usize) {
        match self.robots[r].time_to_empty_s() {
            Some(s) => self.schedule(r, self.now + ms(s), |robot, generation| Ev::Depleted {
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
            let requested: f64 = self
                .sessions
                .iter()
                .filter(|s| matches!(s.source, Source::Station(_)))
                .map(|s| s.power_w)
                .sum();
            let f = self.grid.settle(&self.env, t0, t1, requested);
            let dt_h = secs(t1 - t0) / 3600.0;
            for i in 0..self.sessions.len() {
                let s = self.sessions[i].clone();
                let offered = match s.source {
                    Source::Station(_) => s.power_w * f * dt_h,
                    Source::Mobile(c) => (s.power_w * dt_h).min(self.robots[c].tank_wh),
                };
                let given = self.give_energy(&s, offered);
                if let Source::Mobile(c) = s.source {
                    self.robots[c].tank_wh -= given;
                }
            }
            self.last_fraction = f;
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

    pub(crate) fn start_station_session(
        &mut self,
        r: usize,
        station: usize,
        until_wh: Option<f64>,
    ) {
        self.settle_power();
        let robot = &mut self.robots[r];
        robot.settle_drain(self.now);
        robot.drain_since = None;
        robot.status = RobotStatus::Charging;
        let cap = robot.spec.battery_capacity_wh;
        let fill_tank = until_wh.is_none() && robot.tank_capacity_wh() > 0.0;
        let st = &mut self.stations[station];
        st.occupants.push(r);
        let power_w = st.spec.max_power_w / f64::from(st.spec.slots.max(1));
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
        self.robots[charger].locked_by = Some(r);
        let robot = &mut self.robots[r];
        robot.settle_drain(self.now);
        robot.drain_since = None;
        robot.status = RobotStatus::Charging;
        let cap = robot.spec.battery_capacity_wh;
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
                Source::Mobile(c) => self.robots[c].locked_by = None,
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
                robot.battery_wh = robot.battery_wh.max(s.target_battery_wh);
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
        let robot = &mut self.robots[r];
        robot.settle_drain(self.now);
        robot.battery_wh = 0.0;
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
            robot.status = RobotStatus::Offline;
            robot.drain_since = None;
            self.unschedule(r);
            self.decision_due = true;
        }
    }
}
