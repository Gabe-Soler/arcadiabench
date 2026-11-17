<!-- A leaderboard entry. docs/leaderboard.md has the rules. Open the pull request with ?template=leaderboard-entry.md to get this text. -->

**Entry:** `leaderboard/entries/<id>/<version>/entry.json`

- [ ] The source repository is public, under the licence the entry names.
- [ ] `source.commit` is a full commit hash, and the Dockerfile at that commit builds an image whose entrypoint is the coordinator.
- [ ] The coordinator is deterministic: the same seed gives the same run.
- [ ] It ran in the sandbox on the public seeds: `arcadiabench suite --seeds 1-50 -- leaderboard/tools/sandbox.sh IMAGE`.
- [ ] This pull request adds the entry file and nothing else.

**What it does, in a sentence or two:**
