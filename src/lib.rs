//! ArcadiaBench: an open, seeded benchmark for coordinating robot crews building on Mars.
//!
//! Week-1 scaffold: the coordinator harness ([`coordinator`]), the run event
//! log ([`log`]) and a placeholder scenario ([`scenario`]) wired together by
//! [`run::run_placeholder`]. The discrete-event simulator arrives in week 2.

pub mod coordinator;
pub mod log;
pub mod run;
pub mod scenario;

use arcadia_protocol::SoftwareInfo;

pub fn software_info() -> SoftwareInfo {
    SoftwareInfo {
        name: "arcadiabench".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}
