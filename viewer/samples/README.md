# Sample event logs

Two runs of the week-2 simulator (ArcadiaBench 0.1.0, protocol 0.1.0) for trying the replay viewer. Both are gzip-compressed JSONL event logs, as written by `arcadiabench run --log`.

| File | Run | Size | SHA-256 of the uncompressed log |
|---|---|---|---|
| `builder-seed34.jsonl.gz` | Scripted builder, seed 34: the pad is built and verified in 1.74 sols, through a dust storm that starts on sol 0 and peaks at optical depth 3.4 | 333 KB (1.94 MB raw) | `912fd87a7b43b8d48263e7286497c77afe7f267ca08092d0fb3b8dda7a5f7101` |
| `null-seed7.jsonl.gz` | Null coordinator, seed 7: nothing is commanded, the robots idle until their batteries run out, and the run hits the 60-sol time limit | 200 KB (1.24 MB raw) | `c6b0fb4deda53d79d0d72a38f090bf64b1c67798bfc9d8c70492876e18ee794d` |

Open them with `?log=samples/builder-seed34.jsonl.gz` or `?log=samples/null-seed7.jsonl.gz`.

The simulator is deterministic, so the commands below reproduce both logs byte for byte. Paths are relative to the repository root, at the commit that merged week 2 (`ffa8a66`).

## Null coordinator, seed 7

```sh
cargo build --release
./target/release/arcadiabench run --seed 7 --log null-seed7.jsonl -- ./target/release/null-coordinator
gzip -9 -n -c null-seed7.jsonl > viewer/samples/null-seed7.jsonl.gz
```

## Scripted builder, seed 34

The scripted builder (`tests/support/builder.rs`) runs in process, so it has no command-line entry point. The sample came from a temporary integration test, `tests/viewer_sample.rs`, which was deleted afterwards:

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

Seed 34 was chosen from seeds 1 to 120 because its weather brings a dust storm during the build, so solar output visibly collapses on the second day. Terrain and weather depend only on the seed, so the null coordinator can screen seeds quickly.

`gzip -9 -n` leaves the file name and timestamp out of the header. Other gzip implementations may compress to different bytes, but the uncompressed logs match the hashes above.

## Known gap in these logs

When a run ends, the simulator stops before sending its last observation, so the final events are not logged. In the builder sample, the acceptance and result of the closing verification scan, and a final truth record, are missing. The viewer treats commands issued at the last decision point as running to the end. On `goals_complete` it counts the pad as verified at the end, because the simulator only ends a run that way after verification.
