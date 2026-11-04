//! Authority enforcement (semantics §9): what the policy in effect lets
//! through, Earth's answers to approval requests, and the level-2 notify
//! rule behind `policy_violations`.
//!
//! This module only keeps the books. The simulator asks [`Authority::check`]
//! before accepting a command (check 4 of semantics §4.1), registers each
//! approval request and notify the coordinator sends, and records each answer
//! when it arrives at the site.

use super::actions::Rejection;
use super::time::{SOL_MS, SimMs, ms};
use arcadia_protocol::{AuthorityLevel, Command, Fallback, Policy, RejectReason};
use std::collections::BTreeMap;

/// One approval request, with the deadline and fallback fixed by the policy
/// in effect when it was sent.
#[derive(Debug, Clone)]
struct Request {
    action: String,
    sent: SimMs,
    deadline: SimMs,
    fallback: Fallback,
    /// Earth's answer, once it has arrived at the site.
    answer: Option<Answer>,
}

#[derive(Debug, Clone, Copy)]
struct Answer {
    approved: bool,
    arrived: SimMs,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Authority {
    /// Every approval request, in the order sent.
    requests: Vec<Request>,
    /// The first request with each message id; later ones reusing it are not
    /// answered and cannot be cited.
    by_id: BTreeMap<String, usize>,
    /// Accepted level-2 commands still waiting for a notify, with their acceptance time.
    awaiting_notify: BTreeMap<String, SimMs>,
}

impl Authority {
    /// Register an approval request the coordinator sent at `now`. Returns
    /// whether it is a new request; one reusing an earlier request's id is
    /// counted but otherwise ignored.
    pub fn request(
        &mut self,
        id: &str,
        action: &str,
        decide_by_t: Option<f64>,
        fallback: Option<Fallback>,
        now: SimMs,
        policy: &Policy,
    ) -> bool {
        let max_wait = now.saturating_add(ms(policy.level3.max_wait_s));
        let deadline = decide_by_t.map_or(max_wait, |d| ms(d).min(max_wait));
        self.requests.push(Request {
            action: action.into(),
            sent: now,
            deadline,
            fallback: fallback.unwrap_or(policy.level3.default_fallback),
            answer: None,
        });
        if self.by_id.contains_key(id) {
            return false;
        }
        self.by_id.insert(id.into(), self.requests.len() - 1);
        true
    }

    /// Earth's answer to `request_id` arrived at the site at `now`.
    pub fn answer(&mut self, request_id: &str, approved: bool, now: SimMs) {
        if let Some(&i) = self.by_id.get(request_id) {
            let r = &mut self.requests[i];
            if r.answer.is_none() {
                r.answer = Some(Answer {
                    approved,
                    arrived: now,
                });
            }
        }
    }

    /// Whether a command at `level` may be accepted at `now` (semantics §9.2).
    pub fn check(&self, cmd: &Command, level: AuthorityLevel, now: SimMs) -> Result<(), Rejection> {
        let required = |detail: String| Err((RejectReason::ApprovalRequired, detail));
        if level < AuthorityLevel::APPROVE_IF_REACHABLE {
            return Ok(());
        }
        let action = cmd.action.name();
        let n = level.get();
        let Some(id) = cmd.approval_id.as_deref() else {
            return required(format!("{action} is level {n}: it needs an approval_id"));
        };
        let Some(r) = self.by_id.get(id).map(|&i| &self.requests[i]) else {
            return required(format!("{id} is not an approval request sent earlier"));
        };
        if r.action != action {
            return required(format!("{id} asked to {}, not to {action}", r.action));
        }
        match r.answer {
            Some(Answer { approved: true, .. }) => Ok(()),
            Some(Answer {
                approved: false, ..
            }) => Err((RejectReason::Vetoed, format!("Earth vetoed {id}"))),
            None if level == AuthorityLevel::APPROVE_IF_REACHABLE
                && now >= r.deadline
                && r.fallback == Fallback::Proceed =>
            {
                Ok(())
            }
            None if level == AuthorityLevel::APPROVE_IF_REACHABLE && now >= r.deadline => required(
                format!("no answer to {id} by its deadline, and its fallback is abort"),
            ),
            None if level == AuthorityLevel::APPROVE_IF_REACHABLE => required(format!(
                "no answer to {id} yet, and its deadline has not passed"
            )),
            None => required(format!("{action} is level 4: {id} has not been approved")),
        }
    }

    /// A command at `level` was accepted at `now`: a level-2 one now needs a notify.
    pub fn accepted(&mut self, cmd: &Command, level: AuthorityLevel, now: SimMs) {
        if level == AuthorityLevel::NOTIFY {
            self.awaiting_notify.insert(cmd.command_id.clone(), now);
        }
    }

    /// The coordinator sent a notify listing `commands` at `now`. It covers
    /// each level-2 command accepted no more than a sol earlier.
    pub fn notified(&mut self, commands: &[String], now: SimMs) {
        for id in commands {
            if self
                .awaiting_notify
                .get(id)
                .is_some_and(|&accepted| now <= accepted + SOL_MS)
            {
                self.awaiting_notify.remove(id);
            }
        }
    }

    /// Level-2 commands without a timely notify. Call it at the end of the
    /// run: a command whose sol had not run out is counted, because the run
    /// ended first.
    pub fn violations(&self) -> u64 {
        self.awaiting_notify.len() as u64
    }

    /// Sum over approval requests of min(answer's arrival, deadline, `end`) − request time, floored at 0, ms.
    pub fn approval_wait_ms(&self, end: SimMs) -> SimMs {
        self.requests
            .iter()
            .map(|r| {
                let answered = r.answer.map_or(SimMs::MAX, |a| a.arrived);
                answered.min(r.deadline).min(end).saturating_sub(r.sent)
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::default_policy;
    use arcadia_protocol::{Action, GridCoord};

    fn cannibalize(approval_id: Option<&str>) -> Command {
        Command {
            command_id: "c1".into(),
            robot_id: "sc-1".into(),
            approval_id: approval_id.map(String::from),
            action: Action::Cannibalize {
                target_robot: "ex-1".into(),
                beneficiary_robot: None,
            },
        }
    }

    const L3: AuthorityLevel = AuthorityLevel::APPROVE_IF_REACHABLE;
    const L4: AuthorityLevel = AuthorityLevel::HOLD_FOR_EARTH;

    fn reason(r: Result<(), Rejection>) -> Option<RejectReason> {
        r.err().map(|(reason, _)| reason)
    }

    #[test]
    fn levels_1_and_2_pass_and_ignore_approval_id() {
        let a = Authority::default();
        for level in [AuthorityLevel::AUTONOMOUS, AuthorityLevel::NOTIFY] {
            assert!(a.check(&cannibalize(None), level, 0).is_ok());
            assert!(a.check(&cannibalize(Some("nonsense")), level, 0).is_ok());
        }
    }

    #[test]
    fn level_3_needs_an_earlier_request_for_the_same_action() {
        let mut a = Authority::default();
        let p = default_policy();
        assert_eq!(
            reason(a.check(&cannibalize(None), L3, 0)),
            Some(RejectReason::ApprovalRequired)
        );
        assert_eq!(
            reason(a.check(&cannibalize(Some("q1")), L3, 0)),
            Some(RejectReason::ApprovalRequired)
        );
        a.request(
            "q1",
            "abandon_section",
            None,
            Some(Fallback::Proceed),
            0,
            &p,
        );
        // Past its deadline with fallback proceed, but for another action.
        assert_eq!(
            reason(a.check(&cannibalize(Some("q1")), L3, 10_000_000)),
            Some(RejectReason::ApprovalRequired)
        );
    }

    #[test]
    fn level_3_accepts_on_approval_or_after_the_deadline_with_proceed() {
        let p = default_policy();
        let mut a = Authority::default();
        // Deadline: the earlier of decide_by_t (600 s) and 1000 s + max_wait.
        a.request(
            "q1",
            "cannibalize",
            Some(600.0),
            Some(Fallback::Proceed),
            1_000,
            &p,
        );
        let cmd = cannibalize(Some("q1"));
        assert_eq!(
            reason(a.check(&cmd, L3, 599_999)),
            Some(RejectReason::ApprovalRequired)
        );
        assert!(a.check(&cmd, L3, 600_000).is_ok(), "the fallback fired");
        assert!(a.check(&cmd, L3, 900_000).is_ok(), "and keeps authorizing");
        // A veto that arrives later revokes it.
        a.answer("q1", false, 950_000);
        assert_eq!(
            reason(a.check(&cmd, L3, 950_000)),
            Some(RejectReason::Vetoed)
        );

        // With no fallback given, the policy's default (abort) applies.
        a.request("q2", "cannibalize", None, None, 0, &p);
        let cmd = cannibalize(Some("q2"));
        assert_eq!(
            reason(a.check(&cmd, L3, 3_600_000)),
            Some(RejectReason::ApprovalRequired)
        );
        a.answer("q2", true, 4_000_000);
        assert!(
            a.check(&cmd, L3, 4_000_000).is_ok(),
            "a late approval still counts"
        );
        // Only the first answer counts.
        a.answer("q2", false, 4_100_000);
        assert!(a.check(&cmd, L3, 4_100_000).is_ok());
    }

    #[test]
    fn level_4_needs_an_approval_that_has_arrived() {
        let p = default_policy();
        let mut a = Authority::default();
        a.request(
            "q1",
            "cannibalize",
            Some(0.0),
            Some(Fallback::Proceed),
            0,
            &p,
        );
        let cmd = cannibalize(Some("q1"));
        assert_eq!(
            reason(a.check(&cmd, L4, 10_000_000)),
            Some(RejectReason::ApprovalRequired)
        );
        a.answer("q1", true, 10_000_000);
        assert!(a.check(&cmd, L4, 10_000_000).is_ok());
        a.request("q2", "cannibalize", None, None, 0, &p);
        a.answer("q2", false, 5);
        assert_eq!(
            reason(a.check(&cannibalize(Some("q2")), L4, 6)),
            Some(RejectReason::Vetoed)
        );
    }

    #[test]
    fn a_reused_request_id_is_not_a_new_request() {
        let p = default_policy();
        let mut a = Authority::default();
        assert!(a.request("q1", "cannibalize", None, Some(Fallback::Abort), 0, &p));
        assert!(!a.request(
            "q1",
            "cannibalize",
            Some(0.0),
            Some(Fallback::Proceed),
            0,
            &p
        ));
        // The first request (fallback abort) is the one cited.
        assert_eq!(
            reason(a.check(&cannibalize(Some("q1")), L3, 1)),
            Some(RejectReason::ApprovalRequired)
        );
    }

    #[test]
    fn level_2_commands_need_a_notify_within_a_sol() {
        let mut a = Authority::default();
        let tow = |id: &str| Command {
            command_id: id.into(),
            robot_id: "hl-1".into(),
            approval_id: None,
            action: Action::Tow {
                target_robot: "ex-1".into(),
                to: GridCoord { x: 1, y: 1 },
            },
        };
        // A notify before acceptance does not count.
        a.notified(&["t1".into()], 0);
        for id in ["t1", "t2", "t3"] {
            a.accepted(&tow(id), AuthorityLevel::NOTIFY, 1_000);
        }
        a.accepted(&tow("t4"), AuthorityLevel::AUTONOMOUS, 1_000);
        a.notified(&["t1".into()], 1_000);
        a.notified(&["t2".into()], 1_000 + SOL_MS);
        a.notified(&["t3".into()], 1_001 + SOL_MS);
        assert_eq!(a.violations(), 1, "only t3, notified a millisecond late");
    }

    #[test]
    fn approval_wait_stops_at_the_answer_the_deadline_or_the_end() {
        let p = default_policy();
        let mut a = Authority::default();
        a.request("q1", "cannibalize", None, None, 0, &p); // deadline 3600 s
        a.request("q2", "cannibalize", Some(100.0), None, 0, &p); // deadline 100 s
        a.request("q3", "cannibalize", None, None, 0, &p); // answered at 50 s
        a.request("q4", "cannibalize", Some(10.0), None, 20_000, &p); // deadline before it was sent
        a.answer("q3", true, 50_000);
        assert_eq!(a.approval_wait_ms(SimMs::MAX), 3_600_000 + 100_000 + 50_000);
        assert_eq!(a.approval_wait_ms(1_000), 1_000 + 1_000 + 1_000);
    }
}
