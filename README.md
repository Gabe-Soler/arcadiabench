# ArcadiaBench

ArcadiaBench: an open, seeded benchmark for coordinating robot crews building on Mars.

ArcadiaBench is a fast, headless simulator of a construction site at Arcadia Planitia. Any team can plug in a coordinator and compare results. Coordinators connect through [arcadia-protocol](https://github.com/Gabe-Soler/arcadia-protocol), newline-delimited JSON over stdio, so they can be written in any language. [ArcadiaOS](https://github.com/Gabe-Soler/arcadiaos) is the reference coordinator.

## Status

**Week 1 scaffold.** Working so far:

- **Coordinator harness:** launches any coordinator as a subprocess and does the version handshake. Every message the coordinator sends is validated against the protocol's JSON Schemas, and it rejects malformed or out-of-order replies with precise errors.
- **Run event log:** JSONL, conforming to `event_log.schema.json`. It's the input for scoring and the replay viewer.
- **Placeholder scenario:** one observation at t = 0 (comms up plus the landing-pad goal) and one decision, then the run ends.
- **`null-coordinator`:** a baseline that never acts.

The discrete-event simulator (grid world, capability-based robots, landing-pad task graph, hybrid power) arrives in week 2.

## Run

```sh
cargo build --release
./target/release/arcadiabench run --seed 42 --log run.jsonl -- ./target/release/null-coordinator
./target/release/arcadiabench run --seed 42 --log run.jsonl -- path/to/arcadiaos
```

Everything after `--` is the coordinator command line. The coordinator's stderr is passed through.

## Develop

```sh
cargo test     # harness end-to-end: null coordinator, determinism, and misbehaving coordinators
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

## License

MIT
