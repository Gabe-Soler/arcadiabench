# ArcadiaBench

ArcadiaBench: an open, seeded benchmark for coordinating robot crews building on Mars.

ArcadiaBench is a fast, headless simulator of a construction site at Arcadia Planitia. Any team can plug in a coordinator and compare results. Coordinators connect through [arcadia-protocol](https://github.com/Gabe-Soler/arcadia-protocol), newline-delimited JSON over stdio, so they can be written in any language. [ArcadiaOS](https://github.com/Gabe-Soler/arcadiaos) is the reference coordinator.

## Status

**Week 5: faults and energy.** A coordinator can build a landing pad end to end while robots wear out, break down and get stuck, on a choice of power systems.

- **Scenario `landing-pad-small`:** a seeded 60 × 60 m site (1 m cells) on a gently tilted plain, with variable regolith and a sparse boulder field. The goal is a 20 × 20 m pad with a 12 m blast berm. The crew is four robots defined purely by capabilities, with one charge station, one depot and a comms base station. The coordinator starts with noisy orbital elevation data; scans reveal the ground truth, with sensor noise.
- **Wear and faults:** every robot carries hidden wear on six components (wheels, drive motor, joints, tool, sensors, battery). It grows with distance, work and dust, faster in storms, and makes a robot slower and hungrier. Noisy health telemetry (wheel slip, motor current, dust coverage, temperature) hints at it. Three failure processes run on their own seeded streams: wear-driven hazards that climb steeply with wear, random failures at a base rate, and environmental incidents (sensors knocked out by dust storms, robots stuck in soft regolith). Faults are degraded or disabling, and a disabling one may need the depot.
- **Four ways out:** field `repair` by a repair-capable robot, `tow` to a depot or station, `service` at the depot (clears every fault and resets wear), and `cannibalize` (strip a dead robot for parts; it is lost for good). Cold nights add heater drain that can leave idle robots offline, and an offline robot can be recharged where it stands by a mobile charger. The run ends `fleet_lost` when no robot can ever act again.
- **Run metrics:** every log ends with `run_end.metrics`: completion and sols, pad quality, energy (robot and grid energy, lowest storage, deficit time), fleet health (faults, robot-hours lost, robots lost), Earth traffic and conditions. Every `truth` record carries the true state of the site's power grid, sky and link.
- **Variants:** `arcadiabench run --variant nominal|cascading_faults|solar_only|nuclear_heavy`. `docs/simulator.md` documents every parameter, the functional forms behind them, and the calibration.
- **Discrete-event engine:** integer-millisecond clock and a deterministic event queue. Robot actions take durations and energy drawn from seeded distributions; there are no per-tick physics.
- **Landing-pad task graph, enforced as physics:** each pad cell goes natural → cleared (dig) → graded → compacted → stabilized (sinter). Out-of-order work is rejected, and reworking a cell resets it. Material is conserved: dig spoil fills the robot's payload and then a stockpile, and haul moves real mass.
- **Verification:** the run ends `goals_complete` only when the pad meets spec (surface state, ±5 cm, ≤2° slope, berm height) **and** every pad cell has been scanned since it last changed.
- **Site power:** a fission baseline plus solar (day/night curve × dust transmittance) feeding a site battery bank, with an always-on essential load served first. Station charging draws from the grid and is throttled when the bank runs dry. Mobile chargers carry their own tank. Hybrid is the default; `solar_only` and `nuclear_heavy` are variants.
- **Environment:** sol clock, temperature cycle, and a seeded dust-storm schedule. The coordinator hears about a storm when it starts, peaks and ends, and at every sunrise while it lasts.
- **Fair comparisons:** terrain and weather come from their own seeded streams, so every coordinator on the same seed faces the same world. The whole run is reproducible byte for byte.
- **Coordinator harness:** every message is schema-validated, there's a per-decision wall-clock timeout (`--decision-timeout`, default 10 s), and a guard ends the run if a coordinator freezes sim time.
- **Run event log:** JSONL (`event_log.schema.json`) with ground-truth robot snapshots, changed cells and site state at every decision point and at the very end, the input for scoring and the replay viewer.

**Not yet:**
- Comms delay, blackouts, relay coverage and authority enforcement (week 6). The link is always up, every robot is in coverage, and every command is treated as level 1.
- Scoring, the Small/Medium/Large presets and the full scenario catalog (week 7).

All tunables live in `src/sim/params.rs`, and [`docs/simulator.md`](docs/simulator.md) explains each one. They are plausible placeholders, and sourcing realistic values is still open. The interface semantics the simulator implements are in [arcadia-protocol's `docs/semantics.md`](https://github.com/Gabe-Soler/arcadia-protocol/blob/v0.2.0/docs/semantics.md).

## Run

```sh
cargo build --release
./target/release/arcadiabench run --seed 42 --log run.jsonl -- ./target/release/null-coordinator
./target/release/arcadiabench run --seed 42 --variant solar_only --log run.jsonl -- path/to/arcadiaos
```

Everything after `--` is the coordinator command line. The coordinator's stderr is passed through. A full 60-sol run takes well under a second in release builds. The null coordinator never charges, so its idle robots drain and its run ends `fleet_lost` after about 17 sols.

## Develop

```sh
cargo test     # unit tests, the scripted builder, faults and repair, energy, determinism, week-2 regression, harness robustness
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

`tests/support/builder.rs` is a naive scripted coordinator that builds the pad in about 2 sols using only what it observes, repairing, towing, servicing and recharging robots as faults strike. It's the proof that the default parameters make the mission buildable; with faults turned off it takes about 1.7 sols.

## License

MIT
