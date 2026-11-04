//! ArcadiaBench: an open, seeded benchmark for coordinating robot crews building on Mars.
//!
//! - [`sim`]: the discrete-event simulator (terrain, robots, power, weather,
//!   wear and faults, repair, mission, run metrics). `docs/simulator.md`
//!   documents its parameters.
//! - [`scenario`]: seeded scenario generation.
//! - [`coordinator`]: transports to a coordinator (subprocess or in-process).
//! - [`session`]: one run end to end; [`run`] wraps it for subprocess coordinators.
//! - [`log`]: the JSONL run event log.

pub mod coordinator;
pub mod log;
pub mod run;
pub mod scenario;
pub mod session;
pub mod sim;

use arcadia_protocol::SoftwareInfo;

pub fn software_info() -> SoftwareInfo {
    SoftwareInfo {
        name: "arcadiabench".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}
