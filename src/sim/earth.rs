//! Scripted mission control (semantics §8.3), and the site's side of the
//! Earth link: what happens when the coordinator sends to Earth, when Earth
//! sends, and when Earth's messages arrive at the site.
//!
//! Mission control is deterministic and draws from its own `earth` stream.
//!
//! - **Approval requests.** Earth reads each request when it arrives and
//!   answers it once, after a lognormal human response time, even when the
//!   answer will reach the site after `decide_by_t`. It approves with a
//!   probability that depends on the action ([`approval_probability`]).
//! - **Trust script.** Two rules, each firing at most once. After
//!   `trust_incident_free_sols` without an incident (a fault, a robot
//!   getting stuck, a robot lost) it loosens [`LOOSENED`] from level 2 to
//!   level 1. After a robot is lost it tightens [`TIGHTENED`] to level 4.
//!   Earth learns the site's history from engineering telemetry, which
//!   travels the link like any message, and issues each update
//!   `policy_reaction_s` after it learns what prompts it. Every update
//!   carries the whole policy with `policy_version` one higher.
//! - It never sends `reprioritize`, and `notify` and `status_report` need no answer.

use super::params::Params;
use super::rng::Rng;
use super::time::{SOL_MS, SimMs, ms, secs};
use super::{Ev, Sim};
use arcadia_protocol::*;

/// Level-2 actions the trust script moves to level 1 once trust is earned.
pub const LOOSENED: [&str; 3] = ["tow", "replan", "reassign_task"];

/// The action the trust script moves to level 4 once a robot has been lost.
pub const TIGHTENED: &str = "cannibalize";

/// Mission control's own state.
#[derive(Debug, Clone)]
pub(crate) struct MissionControl {
    rng: Rng,
    /// The policy Earth last sent, starting with the scenario's.
    policy: Policy,
    /// Number of the last message Earth sent (the pre-loaded goal is 1).
    sent: u64,
    /// Bumped by every incident, so an older trust check is ignored.
    trust_generation: u64,
    loosen_planned: bool,
    tighten_planned: bool,
}

impl MissionControl {
    pub fn new(seed: u64, policy: Policy) -> Self {
        Self {
            rng: Rng::stream(seed, "earth"),
            policy,
            sent: 1,
            trust_generation: 0,
            loosen_planned: false,
            tighten_planned: false,
        }
    }
}

/// Something Earth will send.
#[derive(Debug, Clone)]
pub(crate) enum Outgoing {
    Answer { request_id: String, approve: bool },
    Loosen,
    Tighten,
}

/// Chance that Earth approves a request for `action`. `storage_fraction` is
/// the site's true storage, as a fraction of capacity, when the request was sent.
pub fn approval_probability(p: &Params, action: &str, storage_fraction: f64) -> f64 {
    match action {
        "cannibalize" => p.earth_approve_cannibalize_p,
        "abandon_section" => p.earth_approve_abandon_p,
        "commit_energy_reserve" if storage_fraction <= p.earth_reserve_low_frac => {
            p.earth_approve_p
        }
        "commit_energy_reserve" => 0.0,
        _ => p.earth_approve_p,
    }
}

/// `policy` with every action in `actions` at level `from` moved to `to`,
/// and its version bumped; `None` if nothing changes.
fn moved(policy: &Policy, actions: &[&str], from: u8, to: u8) -> Option<Policy> {
    let to = AuthorityLevel::try_from(to).expect("1-4");
    let mut next = policy.clone();
    let mut changed = false;
    for &a in actions {
        if next.level_for(a).get() == from {
            next.levels.insert(a.into(), to);
            changed = true;
        }
    }
    changed.then(|| {
        next.policy_version += 1;
        next
    })
}

impl Sim {
    /// When a message sent now reaches the other end, if before the time limit.
    fn delivery(&self) -> Option<SimMs> {
        let at = self.link.arrival(self.now);
        (at < self.time_limit).then_some(at)
    }

    /// The coordinator sends `m` to Earth now. Returns when it reaches Earth,
    /// if before the time limit.
    pub(crate) fn send_to_earth(&mut self, m: &SiteToEarth) -> Option<SimMs> {
        self.tally.downlink(m);
        let arrival = self.delivery();
        match &m.body {
            Downlink::Notify { commands, .. } => {
                self.authority
                    .notified(commands.as_deref().unwrap_or_default(), self.now);
            }
            Downlink::ApprovalRequest {
                action,
                decide_by_t,
                fallback,
                ..
            } => {
                let new = self.authority.request(
                    &m.message_id,
                    action,
                    *decide_by_t,
                    *fallback,
                    self.now,
                    &self.policy,
                );
                // Earth reads it on arrival and answers once.
                if new && let Some(at) = arrival {
                    let p = &self.params;
                    let capacity = self.grid.bank_capacity_wh;
                    let storage = if capacity > 0.0 {
                        self.site_truth().storage_wh / capacity
                    } else {
                        0.0
                    };
                    let chance = approval_probability(p, action, storage);
                    let (mean, sigma) = (p.earth_response_mean_s, p.earth_response_sigma);
                    let rng = &mut self.earth.rng;
                    let response_s = mean * rng.lognormal_mean1(sigma);
                    let approve = rng.chance(chance);
                    let reply = at.saturating_add(ms(response_s).max(1));
                    if reply < self.time_limit {
                        self.plan_send(
                            reply,
                            Outgoing::Answer {
                                request_id: m.message_id.clone(),
                                approve,
                            },
                        );
                    }
                }
            }
            Downlink::StatusReport { .. } => {}
        }
        arrival
    }

    fn plan_send(&mut self, at: SimMs, what: Outgoing) {
        self.outgoing.push(what);
        self.queue.push(at, Ev::EarthSend(self.outgoing.len() - 1));
    }

    /// Earth sends a message now: it is logged, and travels the link.
    pub(crate) fn on_earth_send(&mut self, i: usize) {
        let body = match self.outgoing[i].clone() {
            Outgoing::Answer {
                request_id,
                approve: true,
            } => Uplink::Approve {
                request_id,
                note: None,
            },
            Outgoing::Answer {
                request_id,
                approve: false,
            } => Uplink::Veto {
                request_id,
                note: Some("vetoed by mission control".into()),
            },
            Outgoing::Loosen => match moved(&self.earth.policy, &LOOSENED, 2, 1) {
                Some(policy) => Uplink::PolicyUpdate { policy },
                None => return,
            },
            Outgoing::Tighten => {
                let below = (1..4).find(|&l| self.earth.policy.level_for(TIGHTENED).get() == l);
                match below.and_then(|l| moved(&self.earth.policy, &[TIGHTENED], l, 4)) {
                    Some(policy) => Uplink::PolicyUpdate { policy },
                    None => return,
                }
            }
        };
        if let Uplink::PolicyUpdate { policy } = &body {
            self.earth.policy = policy.clone();
        }
        self.earth.sent += 1;
        let message = EarthToSite {
            message_id: format!("earth-{:04}", self.earth.sent),
            body,
        };
        let arrival = self.delivery();
        self.uplink_log
            .push((secs(self.now), arrival.map(secs), message.clone()));
        if let Some(at) = arrival {
            self.inbound.push((self.now, message));
            self.queue.push(at, Ev::EarthArrive(self.inbound.len() - 1));
        }
    }

    /// An Earth message arrives at the site: its effect applies now, and the
    /// coordinator hears of it.
    pub(crate) fn on_earth_arrive(&mut self, i: usize) {
        let (sent, message) = self.inbound[i].clone();
        match &message.body {
            Uplink::Approve { request_id, .. } => {
                self.authority.answer(request_id, true, self.now);
            }
            Uplink::Veto { request_id, .. } => {
                self.authority.answer(request_id, false, self.now);
            }
            Uplink::PolicyUpdate { policy } => self.policy = policy.clone(),
            Uplink::Goal { .. } | Uplink::Reprioritize { .. } => {}
        }
        self.emit(
            EventKind::EarthMessage {
                sent_t: secs(sent),
                message,
            },
            true,
        );
    }

    /// A fault, a robot getting stuck or a robot lost: the trust clock restarts.
    pub(crate) fn incident(&mut self) {
        if !self.earth.loosen_planned {
            self.earth.trust_generation += 1;
            self.schedule_trust_check();
        }
    }

    /// The trust script's next check: `trust_incident_free_sols` from now.
    pub(crate) fn schedule_trust_check(&mut self) {
        let wait = ms(self.params.trust_incident_free_sols * secs(SOL_MS));
        let at = self.now.saturating_add(wait);
        if at < self.time_limit {
            let generation = self.earth.trust_generation;
            self.queue.push(at, Ev::TrustCheck { generation });
        }
    }

    /// The site has gone the whole trust period without an incident.
    pub(crate) fn on_trust_check(&mut self, generation: u64) {
        if generation == self.earth.trust_generation && !self.earth.loosen_planned {
            self.earth.loosen_planned = true;
            self.plan_policy_update(Outgoing::Loosen);
        }
    }

    /// A robot was lost.
    pub(crate) fn robot_lost(&mut self) {
        self.incident();
        if !self.earth.tighten_planned {
            self.earth.tighten_planned = true;
            self.plan_policy_update(Outgoing::Tighten);
        }
    }

    /// Earth learns of the site's state now from telemetry, and sends the update after its reaction time.
    fn plan_policy_update(&mut self, what: Outgoing) {
        if let Some(learned) = self.delivery() {
            let at = learned.saturating_add(ms(self.params.policy_reaction_s));
            if at < self.time_limit {
                self.plan_send(at, what);
            }
        }
    }

    /// Messages Earth has sent since the last call, as (send time, arrival
    /// at the site if before the time limit, message), for the event log.
    pub fn take_uplinks(&mut self) -> Vec<(SimTime, Option<SimTime>, EarthToSite)> {
        std::mem::take(&mut self.uplink_log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::default_policy;

    #[test]
    fn approval_odds_depend_on_the_action() {
        let p = Params::default();
        assert_eq!(approval_probability(&p, "cannibalize", 1.0), 0.75);
        assert_eq!(approval_probability(&p, "abandon_section", 1.0), 0.6);
        assert_eq!(approval_probability(&p, "replan", 1.0), 0.9);
        // The reserve is approved only when storage really is low.
        assert_eq!(approval_probability(&p, "commit_energy_reserve", 0.6), 0.0);
        assert_eq!(approval_probability(&p, "commit_energy_reserve", 0.2), 0.9);
    }

    #[test]
    fn the_trust_script_moves_levels_and_bumps_the_version() {
        let p = default_policy();
        let loose = moved(&p, &LOOSENED, 2, 1).expect("tow is level 2");
        assert_eq!(loose.policy_version, p.policy_version + 1);
        for a in LOOSENED {
            assert_eq!(loose.level_for(a), AuthorityLevel::AUTONOMOUS, "{a}");
        }
        assert_eq!(
            loose.level_for("abandon_section"),
            p.level_for("abandon_section")
        );
        assert_eq!(
            moved(&loose, &LOOSENED, 2, 1),
            None,
            "nothing left to loosen"
        );
        let tight = moved(&loose, &[TIGHTENED], 3, 4).unwrap();
        assert_eq!(tight.level_for(TIGHTENED), AuthorityLevel::HOLD_FOR_EARTH);
        assert_eq!(tight.policy_version, p.policy_version + 2);
    }
}
