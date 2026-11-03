# ArcadiaBench

ArcadiaBench: an open, seeded benchmark for coordinating robot crews building on Mars.

ArcadiaBench is a fast, headless simulator of a construction site at Arcadia Planitia. Any team can plug in a coordinator and compare results. Coordinators connect through [arcadia-protocol](https://github.com/Gabe-Soler/arcadia-protocol), newline-delimited JSON over stdio, so they can be written in any language. [ArcadiaOS](https://github.com/Gabe-Soler/arcadiaos) is the reference coordinator.

## Status

**Week 2: core simulator.** A coordinator can build a landing pad end to end.

- **Scenario `landing-pad-small`:** a seeded 60 × 60 m site (1 m cells) on a gently tilted plain, with variable regolith and a sparse boulder field. The goal is a 20 × 20 m pad with a 12 m blast berm. The crew is four robots defined purely by capabilities, with one charge station and one depot. The coordinator starts with noisy orbital elevation data; scans reveal the ground truth, with sensor noise.
- **Discrete-event engine:** integer-millisecond clock and a deterministic event queue. Robot actions take durations and energy drawn from seeded distributions; there are no per-tick physics.
- **Landing-pad task graph, enforced as physics:** each pad cell goes natural → cleared (dig) → graded → compacted → stabilized (sinter). Out-of-order work is rejected, and reworking a cell resets it. Material is conserved: dig spoil fills the robot's payload and then a stockpile, and haul moves real mass.
- **Verification:** the run ends `goals_complete` only when the pad meets spec (surface state, ±5 cm, ≤2° slope, berm height) **and** every pad cell has been scanned since it last changed.
- **Hybrid power:** a fission baseline plus solar (day/night curve × dust transmittance) feeding a site battery bank. Station charging draws from the grid and is throttled when the bank runs dry. Mobile chargers carry their own tank.
- **Environment:** sol clock, temperature cycle, and a seeded dust-storm schedule. The coordinator hears about a storm when it starts, peaks and ends, and at every sunrise while it lasts.
- **Fair comparisons:** terrain and weather come from their own seeded streams, so every coordinator on the same seed faces the same world. The whole run is reproducible byte for byte.
- **Coordinator harness:** every message is schema-validated, there's a per-decision wall-clock timeout (`--decision-timeout`, default 10 s), and a guard ends the run if a coordinator freezes sim time.
- **Run event log:** JSONL (`event_log.schema.json`) with ground-truth robot snapshots and changed cells at every decision point, the input for scoring and the replay viewer.

**Not yet:**
- Faults, wear, and repair/tow/cannibalize (week 5). Those commands are validated but rejected until robots can break.
- Comms delay, blackouts and relay coverage (week 6). The link is always up and every robot is in coverage.
- Scoring and the Small/Medium/Large presets (week 7).

All tunables live in `src/sim/params.rs`. They are plausible placeholders, and sourcing realistic values is still open.

## Run

```sh
cargo build --release
./target/release/arcadiabench run --seed 42 --log run.jsonl -- ./target/release/null-coordinator
./target/release/arcadiabench run --seed 42 --log run.jsonl -- path/to/arcadiaos
```

Everything after `--` is the coordinator command line. The coordinator's stderr is passed through. A 60-sol run takes well under a second in release builds.

## Develop

```sh
cargo test     # unit tests, a scripted coordinator that builds the pad, determinism, harness robustness
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

`tests/support/builder.rs` is a naive scripted coordinator that builds the pad in about 1.6 sols using only what it observes. It's the proof that the default parameters make the mission buildable.

## License

MIT
