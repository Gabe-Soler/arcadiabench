# Sample event logs

Four gzip-compressed JSONL event logs for trying the replay viewer: three runs on protocol 0.2 and one on protocol 0.1, which keeps older logs covered.

| File | Run | Size | SHA-256 of the uncompressed log |
|---|---|---|---|
| `arcadiaos-conjunction-seed13.jsonl.gz` | ArcadiaOS, `conjunction`, seed 13. The conjunction blackout begins at sol 0.75. During it, cp-1's battery fails (disabled, depot only); hl-1 tows it to the depot, where it is serviced and returns to work. The notifies to Earth wait at Mars behind the dark link. The pad is finished and verified at sol 0.88. | 605 KB (5.62 MB raw) | `1e68f992614a455b92d94754c3ef45386db0ec7b6c6b2e8bc6afad64dd6266ad` |
| `arcadiaos-cascading-faults-seed2.jsonl.gz` | ArcadiaOS, `cascading_faults`, seed 2. ex-1's tool fails at sol 0.17; hl-1 tows it to the depot (it shows as being towed), it is serviced, and sc-1 makes five field repairs to cp-1. Finished at sol 0.86. | 583 KB (5.39 MB raw) | `f39b31cd9c4709205e894c151be2cb796b804aa23ab60023bbbbd4e4379cff96` |
| `null-conjunction-seed7.jsonl.gz` | Null coordinator, `conjunction`, seed 7. Nothing is commanded. A 14-sol conjunction blackout, a later outage, and a policy update from Earth; the run ends `fleet_lost` at sol 17.35 once cold nights have drained every battery. | 200 KB (1.11 MB raw) | `6450a9b70bb95e9f2398826f94d1bcfe9d232b17ade7e9d9a8af782be1f857d1` |
| `builder-seed34.jsonl.gz` | Protocol 0.1. The week-2 scripted builder, seed 34: the pad is built and verified in 1.74 sols, through a dust storm that starts on sol 0 and peaks at optical depth 3.4. | 333 KB (1.94 MB raw) | `912fd87a7b43b8d48263e7286497c77afe7f267ca08092d0fb3b8dda7a5f7101` |

Together they take 1.72 MB. Open one with `?log=samples/<file>`, for example `?log=samples/arcadiaos-conjunction-seed13.jsonl.gz&t=68300`, just before the fault.

The simulator and both coordinators are deterministic, so the commands below reproduce the logs byte for byte. `gzip -9 -n` leaves the file name and timestamp out of the header. Other gzip implementations may compress to different bytes, but the uncompressed logs match the hashes above.

## Protocol 0.2 runs

Made with ArcadiaBench at commit `7c41a92` (week 6, protocol 0.2.0) and ArcadiaOS at commit `526b929` (week 6), each built with `cargo build --release`. From the ArcadiaBench repository, with `ARCADIAOS` pointing at ArcadiaOS's `target/release/arcadiaos`:

```sh
./target/release/arcadiabench run --seed 13 --variant conjunction --log arcadiaos-conjunction-seed13.jsonl -- "$ARCADIAOS" < /dev/null
./target/release/arcadiabench run --seed 2 --variant cascading_faults --log arcadiaos-cascading-faults-seed2.jsonl -- "$ARCADIAOS" < /dev/null
./target/release/arcadiabench run --seed 7 --variant conjunction --log null-conjunction-seed7.jsonl -- ./target/release/null-coordinator < /dev/null
for f in arcadiaos-conjunction-seed13 arcadiaos-cascading-faults-seed2 null-conjunction-seed7; do
  gzip -9 -n -c "$f.jsonl" > "viewer/samples/$f.jsonl.gz"
done
```

ArcadiaOS finishes the pad quickly (about 0.8 sols), so the conjunction blackout only shows when it starts inside the run. Conjunction seeds 1 to 60 were screened for a blackout during the run together with a fault; seed 13 has both, and a tow and service. Seed 2 was the `cascading_faults` seed among 1 to 5 with a tow, a depot service and field repairs.

## Protocol 0.1 run

The scripted builder (`tests/support/builder.rs` at the week-2 commit `ffa8a66`) runs in process, so it has no command-line entry point. The sample came from a temporary integration test, `tests/viewer_sample.rs`, which was deleted afterwards:

```rust
mod support;

use arcadiabench::coordinator::InProcess;
use arcadiabench::log::EventLog;
use arcadiabench::session::run_session;
use arcadiabench::sim::params::Params;
use std::fs::File;
use std::io::{BufWriter, Write};
use support::builder::Builder;

#[test]
fn write_builder_log() {
    let seed: u64 = std::env::var("SAMPLE_SEED").unwrap().parse().unwrap();
    let out = std::env::var("SAMPLE_OUT").unwrap();
    let mut link = InProcess::new(Builder::new()).unwrap();
    let mut log = EventLog::new(BufWriter::new(File::create(&out).unwrap()));
    let report = run_session(&mut link, seed, &Params::default(), &mut log).unwrap();
    log.into_inner().flush().unwrap();
    eprintln!("seed {seed}: {report:?}; notes: {:?}", link.agent.notes);
}
```

```sh
SAMPLE_SEED=34 SAMPLE_OUT=$PWD/builder-seed34.jsonl cargo test --release --test viewer_sample -- --nocapture
gzip -9 -n -c builder-seed34.jsonl > viewer/samples/builder-seed34.jsonl.gz
```

Seed 34 was chosen from seeds 1 to 120 because its weather brings a dust storm during the build. Week-2 logs stop at the last decision: the run ends before the simulator's last observation, so the closing scan's acceptance and result and a final truth record are missing. The viewer infers them for such logs; protocol 0.2 simulators write that final instant.
