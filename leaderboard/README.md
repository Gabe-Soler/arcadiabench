# Leaderboard entries

What the front page's leaderboard is drawn from. [`docs/leaderboard.md`](../docs/leaderboard.md) has the rules.

```text
leaderboard/
  entries/<id>/<version>/
    entry.json            who entered what (schemas/leaderboard-entry.schema.json)
    public/report.json    every scorecard and the resilience, on seeds 1-50
    public/runs.json      one score per seed for the nominal scenarios
  board.json              built from the entries; not kept in the repository
  config.json             the catalog, the simulator release and the sandbox's limits
  tools/sandbox.sh        one run's coordinator in its own container
  tools/run-entry.sh      an entry's image through the benchmark, to report.json and runs.json
  tools/dockerfiles/      the baselines' Dockerfiles
```

- **`report.json`** is a report as `arcadiabench score --json` writes it (`schemas/scorecard.schema.json#/$defs/Report`), for that entry alone.
- **`runs.json`** holds, for each nominal scenario, the seeds and each run's score: its speed, or 0 if it did not finish. Two entries are compared on these, seed by seed, to decide whether they share a rank.
- **`board.json`** is what the page reads (`schemas/leaderboard.schema.json`). The Pages workflow builds it when the site is deployed. For a local preview, from the repository root:

  ```sh
  node site/tools/build-board.mjs
  python3 -m http.server 8000     # then open http://localhost:8000/
  ```

## The first entries

ArcadiaOS 1.1.0 and the three baselines, on seeds 1–50 of catalog 1.1.

- Their reports are their scorecards from [`results/v1.1/results.json`](../results/v1.1/results.json), unchanged.
- The entries name the commits that added their Dockerfiles: the baselines' in this repository ([`tools/dockerfiles/`](tools/dockerfiles/)), ArcadiaOS's in its own. Their code is that of release 1.1.0, which is what the published results ran.
- Their per-seed scores come from running the nominal scenarios again, which gives the published scorecards byte for byte:

  ```sh
  arcadiabench bench --catalog 1.1 --preset small --variant nominal --seeds 1-50 --out OUT -- <coordinator>
  ```

  and the same for `medium`, `large`, and for the fleet design track `--crew results/v1.1/fleet/crew.json`. `OUT/metrics.jsonl` has each run's `seed` and `score`.

`tests/leaderboard.rs` checks every entry against its schema, that its per-seed scores average to its scorecards' headlines, and that the board built from the entries matches the board's schema.
