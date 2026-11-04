//! Existing worlds stay the same: on seeds 1-5 the terrain, the scenario
//! (apart from the fields added since) and the weather are byte-identical to
//! what week 2 produced. The fingerprints are FNV-1a hashes of the raw log
//! text of week-2 runs of the null coordinator (`arcadiabench` 0.1.0,
//! protocol 0.1.0), taken without parsing so they do not depend on how JSON
//! numbers are parsed.

use arcadia_protocol::*;
use arcadiabench::coordinator::{Agent, InProcess};
use arcadiabench::log::EventLog;
use arcadiabench::session::run_session;
use arcadiabench::sim::params::Params;

/// (seed, true_site text, scenario text, weather event texts, number of weather events).
const WEEK2: [(u64, u64, u64, u64, usize); 5] = [
    (
        1,
        0xe1580bf28fcba2e5,
        0xc5f993ae54e7c8cc,
        0xbdba886e98765c7a,
        17,
    ),
    (
        2,
        0xfbdc4eb8a339bb5a,
        0xd770939727ce05a4,
        0x322b4d903e7fde6b,
        1,
    ),
    (
        3,
        0x425737f8e58e723c,
        0x8fdfba4cf0431244,
        0xda88f37707b14add,
        9,
    ),
    (
        4,
        0x65a2c189dad7f274,
        0xf81c9e97b296dc89,
        0x8c7981663c23ab3f,
        16,
    ),
    (
        5,
        0x1d8d286c0f59efef,
        0x5b8f88e89b9cb318,
        0xafebf1cbf459957f,
        15,
    ),
];

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The raw text between two markers of a log line.
fn between<'a>(line: &'a str, from: &str, to: &str) -> &'a str {
    let start = line.find(from).expect(from) + from.len();
    let end = start + line[start..].find(to).expect(to);
    &line[start..end]
}

/// Parks every robot on the charge station and keeps it topped up, so the
/// fleet lives through all 60 sols and every storm is reported.
struct Parker {
    capacity: Vec<(String, f64)>,
    next: u64,
}

impl Agent for Parker {
    fn welcome(&mut self, hello: &Hello) -> SoftwareInfo {
        self.capacity = hello
            .scenario
            .robots
            .iter()
            .map(|r| (r.robot_id.clone(), r.battery_capacity_wh))
            .collect();
        SoftwareInfo {
            name: "parker".into(),
            version: "0".into(),
        }
    }

    fn decide(&mut self, obs: &Observation) -> Decision {
        let mut d = Decision::empty(obs.seq);
        for r in &obs.robots {
            let cap = self
                .capacity
                .iter()
                .find(|(id, _)| *id == r.robot_id)
                .map_or(0.0, |(_, c)| *c);
            if r.current_command.is_none()
                && matches!(r.status, RobotStatus::Idle | RobotStatus::Offline)
                && r.battery_wh < 0.9 * cap
            {
                self.next += 1;
                d.commands.push(Command {
                    command_id: format!("p-{}", self.next),
                    robot_id: r.robot_id.clone(),
                    approval_id: None,
                    action: Action::Charge {
                        source: "st-1".into(),
                        until_wh: None,
                    },
                });
            }
        }
        d
    }
}

#[test]
fn seeds_1_to_5_keep_their_week_2_worlds() {
    for (seed, site_fp, scenario_fp, weather_fp, weather_n) in WEEK2 {
        let mut link = InProcess::new(Parker {
            capacity: Vec::new(),
            next: 0,
        })
        .unwrap();
        let mut log = EventLog::new(Vec::new());
        let report = run_session(&mut link, seed, &Params::default(), &mut log).unwrap();
        assert_eq!(report.end_reason, EndReason::TimeLimit, "seed {seed}");
        let text = String::from_utf8(log.into_inner()).unwrap();
        let lines: Vec<&str> = text.lines().collect();

        let start = lines[0];
        let site =
            &start[start.find("\"true_site\":").unwrap() + "\"true_site\":".len()..start.len() - 1];
        assert_eq!(fnv1a(site), site_fp, "seed {seed}: terrain");

        // The scenario minus what v0.2.0 added: power, base_station and the
        // `service` level of the default policy.
        let scenario = between(start, "\"scenario\":", ",\"true_site\":");
        let (head, rest) = scenario.split_at(scenario.find(",\"power\":{").unwrap());
        let tail = &rest[rest.find(",\"policy\":{").unwrap()..];
        let week2_shape = format!("{head}{tail}").replacen("\"service\":1,", "", 1);
        assert_eq!(fnv1a(&week2_shape), scenario_fp, "seed {seed}: scenario");

        let weather: Vec<&str> = lines
            .iter()
            .filter(|l| {
                l.contains("\"kind\":\"observed\",\"event\":{")
                    && l.contains(",\"type\":\"weather\",")
            })
            .map(|l| &l[l.find("\"event\":").unwrap() + "\"event\":".len()..l.len() - 1])
            .collect();
        assert_eq!(weather.len(), weather_n, "seed {seed}: weather events");
        assert_eq!(
            fnv1a(&weather.join("\n")),
            weather_fp,
            "seed {seed}: weather"
        );
    }
}
