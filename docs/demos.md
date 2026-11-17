# Demo replays

ArcadiaOS runs kept as gzipped event logs that the replay viewer opens: five on catalog 1.1 by ArcadiaOS 1.1, and the four on catalog 1.0 by ArcadiaOS 1.0 that were published with v1.0. Each set covers the demo scenarios in the project outline: a nominal build, a dust storm, a conjunction blackout and cascading faults.

Every log is a complete run that ends `goals_complete`, valid against the protocol's `event_log.schema.json`, and under 3 MB. `tests/results.rs` checks all three, and the viewer's tests (`viewer/test/replays.test.js`) replay each one from start to end.

## Catalog 1.1 (ArcadiaOS 1.1)

Builds of weeks, in [`results/v1.1/replays/`](../results/v1.1/replays/). [`results/v1.1/RESULTS.md`](../results/v1.1/RESULTS.md) has the results these runs belong to.

| Demo | Scenario and seed | Log | Size |
|---|---|---|---|
| Nominal build | `landing-pad-1.1-medium-nominal`, seed 1 | [`landing-pad-1.1-medium-nominal-1.jsonl.gz`](../results/v1.1/replays/landing-pad-1.1-medium-nominal-1.jsonl.gz) | 1.6 MB |
| Dust storm | `landing-pad-1.1-large-dust_storm`, seed 1 | [`landing-pad-1.1-large-dust_storm-1.jsonl.gz`](../results/v1.1/replays/landing-pad-1.1-large-dust_storm-1.jsonl.gz) | 2.2 MB |
| Conjunction blackout | `landing-pad-1.1-medium-conjunction`, seed 1 | [`landing-pad-1.1-medium-conjunction-1.jsonl.gz`](../results/v1.1/replays/landing-pad-1.1-medium-conjunction-1.jsonl.gz) | 1.6 MB |
| Cascading faults | `landing-pad-1.1-medium-cascading_faults`, seed 13 | [`landing-pad-1.1-medium-cascading_faults-13.jsonl.gz`](../results/v1.1/replays/landing-pad-1.1-medium-cascading_faults-13.jsonl.gz) | 2.0 MB |
| A crew of its own design | `landing-pad-1.1-crew.2.1.1.10-nominal`, seed 1 | [`landing-pad-1.1-crew.2.1.1.10-nominal-1.jsonl.gz`](../results/v1.1/replays/landing-pad-1.1-crew.2.1.1.10-nominal-1.jsonl.gz) | 1.7 MB |

### Open them in the viewer

The viewer is a static page in `viewer/` that takes the log to open as `?log=`, relative to the page. With the repository served from its root, each replay opens at:

```text
viewer/?log=../results/v1.1/replays/landing-pad-1.1-medium-nominal-1.jsonl.gz
viewer/?log=../results/v1.1/replays/landing-pad-1.1-large-dust_storm-1.jsonl.gz
viewer/?log=../results/v1.1/replays/landing-pad-1.1-medium-conjunction-1.jsonl.gz
viewer/?log=../results/v1.1/replays/landing-pad-1.1-medium-cascading_faults-13.jsonl.gz
viewer/?log=../results/v1.1/replays/landing-pad-1.1-crew.2.1.1.10-nominal-1.jsonl.gz
```

- **Locally:** run `python3 -m http.server 8000` at the repository root and open `http://localhost:8000/` followed by one of the paths above.
- **On GitHub Pages:** the same paths work under the site's address, for example `https://gabe-soler.github.io/arcadiabench/viewer/?log=../results/v1.1/replays/landing-pad-1.1-medium-nominal-1.jsonl.gz`.

Add `&t=` to start at a sim time, in seconds. These runs do not start at `t = 0`: catalog 1.1 starts a run part-way through a sol, at 64,055 s on seed 1.

### What each one shows

- **Nominal build** (Medium, 24.2 sols). Six compactor-sinterers set the pace: each sinters a cell for seven hours and charges for hours in between. ArcadiaOS asks Earth before it charges from the last quarter of the site bank (`t=203353`), and the approval comes back through the passes 20 hours later (`t=276458`). A sinterer's wheels fail in a way that can be repaired in the field (`t=257624`).
- **Dust storm** (Large, 17.7 sols). Ten sinterers want more than the plant gives, and the bank is empty for long stretches of the build. The storm arrives a sol in (`t=159376`) and lasts for the rest of the run, 409 hours, cutting the solar share.
- **Conjunction blackout** (Medium, 24.0 sols). Earth goes out of reach a sol and a half in (`t=197048`), for 14 sols. Under two hours later ArcadiaOS asks to charge from the bank's reserve (`t=203353`); the answer arrives after the blackout ends (`t=1439898`), at `t=1480154`. The build carries on without it and finishes in about the time of the nominal one. A sinterer's joints fail in a way only a depot can fix (`t=339511`), and the crew deals with it with Earth out of reach.
- **Cascading faults** (Medium, 26.3 sols). 130 faults, 38 of them disabling, against 25 in the nominal replay. A hauler's wheels and an excavator's joints fail within the first half sol, and both need the depot (`t=54597`, `t=67174`). Earth vetoes the request to charge from the reserve (`t=209338`). A dust storm crosses the middle of the build (`t=691345` to `t=1525748`).
- **A crew of its own design** (16.2 sols). The fleet design track: 2 scouts, 1 excavator, 1 hauler and 10 compactor-sinterers, the crew `arcadiaos design-crew` chooses for the 10 t budget, on the Medium outpost. With one excavator and one hauler, a sinterer that needs the depot is towed by the same robots that dig and haul (`t=119270`).

### Reproduce

From the root of ArcadiaBench, with ArcadiaOS 1.1 checked out beside it, build both in release and run [`scripts/demos.sh`](../scripts/demos.sh):

```sh
cargo build --release
(cd ../arcadiaos && cargo build --release)
scripts/demos.sh
```

The script runs these five commands, with `O` the ArcadiaOS binary (`$ARCADIAOS`, by default `../arcadiaos/target/release/arcadiaos`), and moves the logs into `results/v1.1/replays/` once all are done:

```sh
B=./target/release
$B/arcadiabench run --catalog 1.1 --seed 1  --preset medium --variant nominal          --log landing-pad-1.1-medium-nominal-1.jsonl.gz -- $O
$B/arcadiabench run --catalog 1.1 --seed 1  --preset large  --variant dust_storm       --log landing-pad-1.1-large-dust_storm-1.jsonl.gz -- $O
$B/arcadiabench run --catalog 1.1 --seed 1  --preset medium --variant conjunction      --log landing-pad-1.1-medium-conjunction-1.jsonl.gz -- $O
$B/arcadiabench run --catalog 1.1 --seed 13 --preset medium --variant cascading_faults --log landing-pad-1.1-medium-cascading_faults-13.jsonl.gz -- $O
$B/arcadiabench run --catalog 1.1 --seed 1  --crew results/v1.1/fleet/crew.json --variant nominal --log landing-pad-1.1-crew.2.1.1.10-nominal-1.jsonl.gz -- $O
```

Runs are deterministic and the gzip stream carries no time or name, so the script rewrites the files byte for byte.

## Catalog 1.0 (ArcadiaOS 1.0)

Four runs of hours, in [`results/v1.0/replays/`](../results/v1.0/replays/), written by ArcadiaBench 1.0.0 on protocol 1.0.0. Together they cover all three presets. [`results/v1.0/RESULTS.md`](../results/v1.0/RESULTS.md) has the results they belong to. `scripts/demos.sh v1.0` runs the same four scenarios with a current ArcadiaBench, which needs a coordinator that speaks its protocol version.

| Demo | Scenario and seed | Log | Size |
|---|---|---|---|
| Nominal build | `landing-pad-large-nominal`, seed 1 | [`landing-pad-large-nominal-1.jsonl.gz`](../results/v1.0/replays/landing-pad-large-nominal-1.jsonl.gz) | 1.1 MB |
| Dust storm | `landing-pad-small-dust_storm`, seed 15 | [`landing-pad-small-dust_storm-15.jsonl.gz`](../results/v1.0/replays/landing-pad-small-dust_storm-15.jsonl.gz) | 0.6 MB |
| Conjunction blackout | `landing-pad-medium-conjunction`, seed 1 | [`landing-pad-medium-conjunction-1.jsonl.gz`](../results/v1.0/replays/landing-pad-medium-conjunction-1.jsonl.gz) | 0.8 MB |
| Cascading faults | `landing-pad-medium-cascading_faults`, seed 13 | [`landing-pad-medium-cascading_faults-13.jsonl.gz`](../results/v1.0/replays/landing-pad-medium-cascading_faults-13.jsonl.gz) | 0.9 MB |

### Open them in the viewer

The viewer is a static page in `viewer/` that takes the log to open as `?log=`, relative to the page. With the repository served from its root, each replay opens at:

```text
viewer/?log=../results/v1.0/replays/landing-pad-large-nominal-1.jsonl.gz
viewer/?log=../results/v1.0/replays/landing-pad-small-dust_storm-15.jsonl.gz
viewer/?log=../results/v1.0/replays/landing-pad-medium-conjunction-1.jsonl.gz
viewer/?log=../results/v1.0/replays/landing-pad-medium-cascading_faults-13.jsonl.gz
```

- **Locally:** run `python3 -m http.server 8000` at the repository root and open `http://localhost:8000/` followed by one of the paths above.
- **On GitHub Pages:** the Viewer Pages workflow publishes `viewer/` and `results/` side by side, so the same paths work under the site's address, for example `https://gabe-soler.github.io/arcadiabench/viewer/?log=../results/v1.0/replays/landing-pad-large-nominal-1.jsonl.gz`.

Add `&t=` to start at a moment: plain seconds, or a unit such as `t=0.25sol`. The moments below are worth a look.

### What each one shows

#### Nominal build: Large, seed 1

Twenty robots build the pad in 0.079 sols (1.9 hours), with no faults, all before sunrise.

- A scout surveys the pad at once. The excavators start digging at 32 s, the haulers move fill from 59 s, and grading, compaction and sintering follow within two minutes, so the eight compactor-sinterers are busy almost from the start.
- The pad fills in roughly in sweep order from the corner nearest the outpost, with the compactor-sinterers spread across it, each compacting a cell and sintering it close by.
- The berm, 3 m beyond the pad and out of the base station's coverage, is built from a row of borrow cells just inside coverage, so no robot ever loses contact.
- The run ends with the final verification scan, seconds after the last sinter.

#### Dust storm: Small, seed 15

Four robots work through a severe regional storm. The pad takes 0.83 sols, against a median of 0.80 for nominal Small runs and 0.825 for this variant.

- The storm front arrives at 909 s (`t=909`). Optical depth climbs to its peak of 5.0 at 3572 s and stays above 4.7 into the day.
- Sunrise at 22,194 s (`t=0.25sol`) brings little sunlight: the arrays give under 2 kW all day, against 20 kW at a clear noon, and the fission baseline carries the site.
- The storm knocks out the sensors of the hauler at 1935 s and of the compactor-sinterer at 5733 s, both degraded. The scout repairs the compactor-sinterer in the field, then keeps repairing it as the storm wears it down, ArcadiaOS's predictive maintenance at work: seven field repairs in all.
- The compactor-sinterer, the bottleneck, stops twice to charge before its last sinter.

#### Conjunction blackout: Medium, seed 1

Ten robots build the pad in 0.145 sols (3.6 hours) while the Earth link goes dark.

- The run starts with the link up and a one-way delay of about 24 minutes, and ArcadiaOS sends its opening status report.
- The link goes down for the solar conjunction at 1183 s (`t=1183`), with `expected_restore_t` 14 sols away.
- Nothing stops: nothing in ArcadiaOS's default policy needs an answer from Earth, so the build carries on to the end in blackout. The crew gets exactly the commands it gets in `landing-pad-medium-nominal` on the same seed.

#### Cascading faults: Medium, seed 13

Ten robots on a site with four to five times the nominal wear and failure rates. Three faults strike, and three different repairs follow; the pad takes 0.158 sols, against a median of 0.153 for the variant.

- At 2902 s a compactor-sinterer's drive motor degrades (`t=2902`). It keeps working, a little slower.
- At 3725 s another compactor-sinterer's joints fail, disabling it (`t=3725`). The fault can be fixed in the field, and a scout repairs it.
- At 9756 s an excavator's battery fails (`t=9756`). That fault needs the depot: a hauler tows the excavator to a depot, ArcadiaOS notifies Earth of the level-2 tow, and the depot services it from 10,006 s while the rest of the crew carries on.

### Reproduce

Check out tag `v1.0.0` of ArcadiaBench, with ArcadiaOS 1.0 checked out beside it, build both in release and run that tag's `scripts/demos.sh`:

```sh
cargo build --release
(cd ../arcadiaos && cargo build --release)
scripts/demos.sh
```

The replays are runs on catalog 1.0, written by ArcadiaBench 1.0.0. The script runs these four commands, with `O` the ArcadiaOS binary (`$ARCADIAOS`, by default `../arcadiaos/target/release/arcadiaos`), and moves the logs into `results/v1.0/replays/` once all four are done (or into the directory it is given):

```sh
B=./target/release
$B/arcadiabench run --catalog 1.0 --seed 1  --preset large  --variant nominal          --log landing-pad-large-nominal-1.jsonl.gz -- $O
$B/arcadiabench run --catalog 1.0 --seed 15 --preset small  --variant dust_storm       --log landing-pad-small-dust_storm-15.jsonl.gz -- $O
$B/arcadiabench run --catalog 1.0 --seed 1  --preset medium --variant conjunction      --log landing-pad-medium-conjunction-1.jsonl.gz -- $O
$B/arcadiabench run --catalog 1.0 --seed 13 --preset medium --variant cascading_faults --log landing-pad-medium-cascading_faults-13.jsonl.gz -- $O
```

Runs are deterministic and the gzip stream carries no time or name, so the script rewrites the four files byte for byte.

### Choosing the runs

Each scenario's preset was picked for what it shows best:

- **Nominal build on Large**, because twenty robots at work show the parallel pipeline most clearly.
- **Dust storm on Small**, because the Small build lasts most of a sol and works through the stormy day. Medium and Large builds end before sunrise, so the storm costs them sensor faults and wear but no sunlight.
- **Conjunction and cascading faults on Medium**, the default.

The seeds are typical rather than extreme. Seed 15 of the dust storm finishes close to the variant's median and shows storm faults. Seed 13 of cascading faults is close to its median and shows the three main ways out of a fault: degraded but working, a field repair, and a tow to the depot for service.
