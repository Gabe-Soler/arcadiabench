# The leaderboard

The [front page](https://gabe-soler.github.io/arcadiabench/#leaderboard) ranks coordinators on ArcadiaBench's catalog 1.1. Every number on it comes from runs the project made itself: no entry reports its own score.

**Where it stands.** The board shows the [published v1.1 results](../results/v1.1/RESULTS.md) on seeds 1–50: ArcadiaOS and the three baselines. The runner that takes outside entries, and the first round on held-out seeds, are the next two steps. Until the runner is merged, an entry can be prepared but not run.

## What ranks

- **The number.** An entry's headline, success rate × speed ([`scoring.md`](scoring.md)), on the nominal variant.
  - **Standard track:** averaged over the Small, Medium and Large outposts.
  - **Fleet design track:** for the one crew the entry brings.
- **Beside it:** each outpost's headline, resilience, how many runs finished, and every scenario's headline when a row is opened.
- **Ties.** Two entries are compared seed by seed on the same runs. If their mean difference is within two standard errors of zero, they share a rank, shown as `2=`. On the published results greedy and random share second place.

## What an entry is

A pull request that adds one file, `leaderboard/entries/<id>/<version>/entry.json`:

```json
{
  "id": "my-coordinator",
  "version": "0.3.0",
  "name": "My coordinator",
  "authors": ["Your name"],
  "licence": "MIT",
  "baseline": false,
  "summary": "One or two sentences on how it works.",
  "source": {
    "repository": "https://github.com/you/my-coordinator",
    "commit": "0123456789abcdef0123456789abcdef01234567"
  },
  "tracks": ["standard"]
}
```

- **Open source.** `repository` is public, and `commit` is a full commit hash, never a branch or a tag.
- **It builds with a Dockerfile** at the repository's root, whose image runs the coordinator: a program that speaks [arcadia-protocol](https://github.com/Gabe-Soler/arcadia-protocol) on standard input and output.
- **It is deterministic.** The same seed gives the same run. An entry that answers differently on a second run is not ranked.
- **The fleet design track** adds `"fleet"` to `tracks` and a `fleet` block with the crew and its mass ([`schemas/crew.schema.json`](../schemas/crew.schema.json)).
- **An entry does not change.** A new version is a new directory.

[`schemas/leaderboard-entry.schema.json`](../schemas/leaderboard-entry.schema.json) is the whole format, and `tests/leaderboard.rs` checks every entry against it.

## How an entry is run

1. **You open the pull request.** A maintainer reads it.
2. **A maintainer starts the run.** The project builds your image once, records its digest, and runs it in containers with no network: one coordinator process per run, the same simulator release for every entry.
3. **Results come back as a pull request** with the scorecards. When it is merged, the board shows your row.

## Rounds and seeds

- **A round is a tag on `main`.** Only entries accepted before the tag are ranked in it.
- **Its seeds are computed from the tag's commit hash.** Nobody can know them while the entries are being written, and nothing has to be kept secret.
- **Anyone can reproduce a round**, byte for byte, the moment it is published: the seeds, the simulator release and every entry's image are public.
- **Seeds 1–50 stay public.** Every entry is also run on them, so you can check your own setup against the board before you enter.
- **An entry that arrives after a round's tag** shows its public-seed figures until the next round.

## What is published

For each entry: its `entry.json`, the report with every scorecard (`report.json`, [`schemas/scorecard.schema.json`](../schemas/scorecard.schema.json)`#/$defs/Report`), and one score per seed for the nominal scenarios (`runs.json`), which is what ties are worked out from. [`leaderboard/README.md`](../leaderboard/README.md) describes the files.
