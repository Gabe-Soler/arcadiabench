//! The three allocation policies: the only thing that differs between the
//! baselines. Each matches ready tasks to free robots that can do them and
//! afford them (the task's energy plus the trip home from where it ends).
//!
//! - **Random:** ready tasks in priority order; each goes to a capable
//!   free robot drawn at random from a generator seeded with the run's seed.
//! - **Greedy:** ready tasks in priority order; each goes to the capable
//!   free robot with the lowest estimated cost: travel time plus work time.
//! - **First come, first served:** robots in the order they became free;
//!   each takes the oldest ready task (the first created) it can do.
//!
//! Priority order is shared: scans that unblock or verify work, then the
//! pad pipeline from its downstream end (sinter, compact, grade, fill,
//! dig), then the berm; within a class, cells in sweep order. Ties go to
//! the lower robot index.

use super::plan::{Hold, Plan};
use super::world::World;
use arcadia_protocol::Action;
use std::fmt;
use std::str::FromStr;

/// How a baseline matches tasks to robots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Allocation {
    Random,
    Greedy,
    Fcfs,
}

impl Allocation {
    pub const ALL: [Allocation; 3] = [Allocation::Random, Allocation::Greedy, Allocation::Fcfs];

    /// The coordinator's name: `baseline-random`, `baseline-greedy` or `baseline-fcfs`.
    pub fn name(self) -> &'static str {
        match self {
            Allocation::Random => "baseline-random",
            Allocation::Greedy => "baseline-greedy",
            Allocation::Fcfs => "baseline-fcfs",
        }
    }
}

impl fmt::Display for Allocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Allocation {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("baseline-").unwrap_or(s);
        Allocation::ALL
            .into_iter()
            .find(|a| a.name().strip_prefix("baseline-") == Some(s))
            .ok_or_else(|| format!("unknown allocation '{s}' (expected random, greedy or fcfs)"))
    }
}

/// SplitMix64: a tiny seeded generator, the same on every platform.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x6261_7365_6c69_6e65) // "baseline"
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// A task given to a robot.
#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub robot: usize,
    pub task: u64,
    pub action: Action,
    pub hold: Hold,
}

/// The estimate of a task for `r`, if `r` can do it and afford it.
fn fits(plan: &Plan, id: u64, r: usize, world: &World) -> Option<f64> {
    plan.estimate(id, r, world)
        .filter(|e| world.affords(r, e.wh, e.end))
        .map(|e| e.secs)
}

fn give(
    plan: &mut Plan,
    id: u64,
    r: usize,
    world: &World,
    free: &mut Vec<usize>,
    out: &mut Vec<Assignment>,
) -> bool {
    match plan.command(id, r, world) {
        Some((action, hold)) => {
            free.retain(|&x| x != r);
            out.push(Assignment {
                robot: r,
                task: id,
                action,
                hold,
            });
            true
        }
        None => false,
    }
}

/// Match `ready` tasks to `free` robots; assigned robots leave `free`.
pub fn allocate(
    how: Allocation,
    world: &World,
    plan: &mut Plan,
    ready: &[u64],
    free: &mut Vec<usize>,
    rng: &mut Rng,
) -> Vec<Assignment> {
    let mut out = Vec::new();
    let now = world.t;
    match how {
        Allocation::Greedy | Allocation::Random => {
            let mut order = ready.to_vec();
            order.sort_by_key(|&id| plan.priority(id));
            for id in order {
                if free.is_empty() {
                    break;
                }
                if !plan.can_start(id, now) {
                    continue;
                }
                let able: Vec<(f64, usize)> = free
                    .iter()
                    .filter_map(|&r| fits(plan, id, r, world).map(|s| (s, r)))
                    .collect();
                let pick = match how {
                    Allocation::Greedy => able
                        .iter()
                        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
                        .map(|&(_, r)| r),
                    _ if able.is_empty() => None,
                    _ => Some(able[rng.below(able.len())].1),
                };
                if let Some(r) = pick {
                    give(plan, id, r, world, free, &mut out);
                }
            }
        }
        Allocation::Fcfs => {
            let mut robots = free.clone();
            robots.sort_by(|&a, &b| {
                world.robots[a]
                    .free_since
                    .total_cmp(&world.robots[b].free_since)
                    .then(a.cmp(&b))
            });
            let mut order = ready.to_vec();
            order.sort_unstable();
            for r in robots {
                let first = order
                    .iter()
                    .copied()
                    .find(|&id| plan.can_start(id, now) && fits(plan, id, r, world).is_some());
                if let Some(id) = first
                    && give(plan, id, r, world, free, &mut out)
                {
                    order.retain(|&x| x != id);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_parse() {
        for a in Allocation::ALL {
            assert_eq!(a.name().parse::<Allocation>().unwrap(), a);
        }
        assert_eq!("greedy".parse::<Allocation>().unwrap(), Allocation::Greedy);
        assert!("smart".parse::<Allocation>().is_err());
    }

    #[test]
    fn the_generator_is_seeded_and_in_range() {
        let a: Vec<usize> = {
            let mut r = Rng::new(7);
            (0..20).map(|_| r.below(5)).collect()
        };
        let b: Vec<usize> = {
            let mut r = Rng::new(7);
            (0..20).map(|_| r.below(5)).collect()
        };
        assert_eq!(a, b);
        assert!(a.iter().all(|&x| x < 5));
        let mut r = Rng::new(8);
        assert_ne!(a, (0..20).map(|_| r.below(5)).collect::<Vec<_>>());
    }
}
