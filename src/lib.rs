//! ArcadiaBench: an open, seeded benchmark for coordinating robot crews building on Mars.
//!
//! - [`sim`]: the discrete-event simulator (terrain, robots, power, weather,
//!   wear and faults, repair, mission, the Earth link, scripted mission
//!   control, authority enforcement, coverage and relays, run metrics).
//!   `docs/simulator.md` documents its parameters.
//! - [`presets`]: the Small, Medium and Large crews and their outposts.
//! - [`scenario`]: seeded scenario generation for every preset and variant.
//! - [`coordinator`]: transports to a coordinator (subprocess or in-process).
//! - [`session`]: one run end to end; [`run`] wraps it for subprocess coordinators.
//! - [`log`]: the JSONL run event log.
//! - [`score`]: scores, scorecards and resilience (`docs/scoring.md`).
//! - [`bench`](mod@bench): seeded batches and suites, run in parallel.
//! - [`cli`]: the `arcadiabench` command line.

pub mod bench;
pub mod cli;
pub mod coordinator;
pub mod log;
pub mod presets;
pub mod run;
pub mod scenario;
pub mod score;
pub mod session;
pub mod sim;

use arcadia_protocol::SoftwareInfo;

pub fn software_info() -> SoftwareInfo {
    SoftwareInfo {
        name: "arcadiabench".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}
