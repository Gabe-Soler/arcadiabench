# The leaderboard

The [front page](https://gabe-soler.github.io/arcadiabench/#leaderboard) ranks coordinators on ArcadiaBench's catalog 1.1. Every number on it comes from runs the project made itself: no entry reports its own score.

**Where it stands.** The board shows the [published v1.1 results](../results/v1.1/RESULTS.md) on seeds 1–50: ArcadiaOS and the three baselines. Entries are open: an entry is built and run on the public seeds by the runner described below. The first round on held-out seeds has not run yet.

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
- **It builds with a Dockerfile**, at the repository's root unless `source.dockerfile` names another path. The image's entrypoint is the coordinator: a program that speaks [arcadia-protocol](https://github.com/Gabe-Soler/arcadia-protocol) on standard input and output. [`leaderboard/tools/dockerfiles/`](../leaderboard/tools/dockerfiles/) has the baselines' as examples.
- **It runs in the sandbox below.** Try it there before you enter: `arcadiabench suite --seeds 1-50 -- leaderboard/tools/sandbox.sh YOUR_IMAGE`.
- **It is deterministic.** The same seed gives the same run. An entry that answers differently on a second run is not ranked.
- **The fleet design track** adds `"fleet"` to `tracks` and a `fleet` block with the crew and its mass ([`schemas/crew.schema.json`](../schemas/crew.schema.json)).
- **An entry does not change.** A new version is a new directory.

[`schemas/leaderboard-entry.schema.json`](../schemas/leaderboard-entry.schema.json) is the whole format, and `tests/leaderboard.rs` checks every entry against it.

## How an entry is run

1. **You open the pull request** with your entry file ([template](../.github/PULL_REQUEST_TEMPLATE/leaderboard-entry.md)). A maintainer reads it and merges it.
2. **A maintainer starts the runner** for it. Nothing runs by itself.
3. **The runner builds your image once**, from the pinned commit, and keeps it in the project's registry under its digest. Every later run uses that exact image.
4. **It runs the image on the public seeds, twice.** The two have to be the same byte for byte. A run that crashes, breaks the protocol or times out fails the entry.
5. **The results come back on a branch**, which a maintainer opens as a pull request. When it is merged, the board shows your row.

### The sandbox

Each run's coordinator is its own container ([`leaderboard/tools/sandbox.sh`](../leaderboard/tools/sandbox.sh)). It gets the protocol on standard input and output, and nothing else:

| | |
|---|---|
| Network | none |
| File system | read-only, with 64 MB at `/tmp` |
| User | 65534, no capabilities, no new privileges |
| Memory | 2 GB, no swap |
| CPU | 1 |
| Processes | 256 |
| A decision | 10 seconds (`--decision-timeout`) |
| A run | 900 seconds of wall clock; a watchdog kills the container after that |
| The image | 1 GB compressed at most |

Building is the only step with network access. The numbers are in [`leaderboard/config.json`](../leaderboard/config.json), with the simulator release every entry is run on.

### For maintainers

- **Once:** in the repository's settings, under *Secrets and variables → Actions → Variables*, set `LEADERBOARD_GIT_NAME` and `LEADERBOARD_GIT_EMAIL`: the results are committed under them. After the first image is pushed, make its package public under the owner's *Packages*, so anyone can pull an entry's image to reproduce it.
- **To run an entry:** *Actions → Leaderboard → Run workflow*, and give the entry as `ID/VERSION`. When it finishes, its summary has the link that opens the results as a pull request.
- **What the workflow is made of** is tested on every push that touches it ([`leaderboard-check.yml`](../.github/workflows/leaderboard-check.yml)): the same runs come out byte for byte in the sandbox and outside it; a coordinator in the sandbox cannot write outside `/tmp` or reach the network; one that hangs is killed, container included; one that is not deterministic gets no results.
- **Submitted code is untrusted.** No job both runs it and holds a permission worth stealing: the build and the runs have none, and the job that pushes the image and the results runs no submitted code. What an entry file says is checked against patterns ([`site/tools/entry-fields.mjs`](../site/tools/entry-fields.mjs)) and reaches git and docker as environment variables, never as text pasted into a command.

## Rounds and seeds

- **A round is a tag on `main`.** Only entries accepted before the tag are ranked in it.
- **Its seeds are computed from the tag's commit hash.** Nobody can know them while the entries are being written, and nothing has to be kept secret.
- **Anyone can reproduce a round**, byte for byte, the moment it is published: the seeds, the simulator release and every entry's image are public.
- **Seeds 1–50 stay public.** Every entry is also run on them, so you can check your own setup against the board before you enter.
- **An entry that arrives after a round's tag** shows its public-seed figures until the next round.

## What is published

For each entry: its `entry.json`, the report with every scorecard (`report.json`, [`schemas/scorecard.schema.json`](../schemas/scorecard.schema.json)`#/$defs/Report`), and one score per seed for the nominal scenarios (`runs.json`), which is what ties are worked out from. [`leaderboard/README.md`](../leaderboard/README.md) describes the files.
