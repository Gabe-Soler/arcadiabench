"""The arcadiabench package: scenarios, runs in process and as subprocesses,
batches, scoring, serve() under the real command line, and errors."""

import gzip
import io
import json
import subprocess
import sys
from importlib.metadata import version
from pathlib import Path

import pytest

import arcadiabench
from arcadiabench import Coordinator, CoordinatorError


def test_the_package_describes_itself():
    assert arcadiabench.PROTOCOL_VERSION == "1.0.0"
    assert arcadiabench.__version__.count(".") == 2
    # The package's version (python/Cargo.toml) is the simulator's (Cargo.toml).
    assert version("arcadiabench") == arcadiabench.__version__
    assert arcadiabench.PRESETS == ["small", "medium", "large"]
    assert arcadiabench.VARIANTS[0] == "nominal" and "dust_storm" in arcadiabench.VARIANTS
    assert set(arcadiabench.STRESS_VARIANTS) == {"dust_storm", "conjunction", "cascading_faults"}
    schema = json.loads(arcadiabench.SCORECARD_SCHEMA)
    assert schema["$schema"] == "https://json-schema.org/draft/2020-12/schema"
    assert (Path(arcadiabench.__file__).parent / "py.typed").exists()


def test_scenario_is_the_hello_of_every_preset_and_variant():
    sizes = {"small": 4, "medium": 10, "large": 20}
    for preset in arcadiabench.PRESETS:
        for variant in arcadiabench.VARIANTS:
            hello = arcadiabench.scenario(7, preset, variant)
            assert hello["type"] == "hello"
            assert hello["protocol_version"] == arcadiabench.PROTOCOL_VERSION
            assert hello["run_id"] == f"landing-pad-{preset}-{variant}-7"
            assert hello["seed"] == 7
            s = hello["scenario"]
            assert s["scenario_id"] == f"landing-pad-{preset}-{variant}"
            assert len(s["robots"]) == sizes[preset]
            assert s["base_station"]["position"] == {"x": 30, "y": 8}
    # One seed is one site, whatever the preset.
    a, b = arcadiabench.scenario(3, "small"), arcadiabench.scenario(3, "large", "solar_only")
    assert a["scenario"]["site"] == b["scenario"]["site"]
    assert a["scenario"]["power"]["mode"] == "hybrid"
    assert b["scenario"]["power"]["mode"] == "solar_only"
    with pytest.raises(ValueError, match="unknown preset"):
        arcadiabench.scenario(1, "huge")
    with pytest.raises(ValueError, match="unknown variant"):
        arcadiabench.scenario(1, "small", "sandstorm")


def test_an_in_process_coordinator_builds_the_pad(builder_module, tmp_path):
    log = tmp_path / "run.jsonl.gz"
    result = arcadiabench.run(builder_module.Builder(), seed=1, log_path=log)
    assert result["run_id"] == "landing-pad-small-nominal-1"
    assert result["coordinator"] == {"name": "python-builder", "version": "1.0.0"}
    assert result["reason"] == "goals_complete" and result["error"] is None
    m = result["metrics"]
    assert m["completed"] and m["quality"]["cells_in_spec"] == m["quality"]["cells_total"] == 400
    assert 0 < result["speed"] <= 1 and result["score"] == result["speed"]
    assert result["speed"] == pytest.approx(result["reference_sols"] / m["sols"])
    assert result["end_t"] == pytest.approx(m["sols"] * 88775.0)
    assert result["progress"] == 1.0 and result["lost_link_returns"] >= 0
    # The log is complete, gzipped, and scores to the same run.
    lines = gzip.decompress(log.read_bytes()).decode().splitlines()
    assert json.loads(lines[0])["kind"] == "run_start"
    assert json.loads(lines[-1])["kind"] == "run_end"
    report = arcadiabench.score(log)
    card = report["scorecards"][0]
    assert card["headline"] == result["score"] and card["seeds"] == [1]


def test_an_external_coordinator_runs_from_its_command_line(builder_argv, tmp_path):
    log = tmp_path / "run.jsonl"
    result = arcadiabench.run(builder_argv, seed=2, preset="medium", log_path=log)
    assert result["scenario_id"] == "landing-pad-medium-nominal"
    assert result["reason"] == "goals_complete"
    assert json.loads(log.read_text().splitlines()[-1])["kind"] == "run_end"
    # The same coordinator in process makes the same run, byte for byte.
    in_process = tmp_path / "in-process.jsonl"
    sys.path.insert(0, str(Path(builder_argv[1]).parent))
    try:
        import builder
    finally:
        sys.path.pop(0)
    again = arcadiabench.run(builder.Builder(), seed=2, preset="medium", log_path=in_process)
    assert again == result
    assert in_process.read_bytes() == log.read_bytes()


def test_serve_runs_under_the_real_command_line(builder_argv, tmp_path):
    log = tmp_path / "cli.jsonl"
    cmd = [sys.executable, "-m", "arcadiabench", "run", "--seed", "3", "--variant", "dust_storm",
           "--log", str(log), "--", *builder_argv]
    # From a neutral directory: `-m` puts the working directory first on the path.
    done = subprocess.run(cmd, capture_output=True, text=True, timeout=300, cwd=tmp_path)
    assert done.returncode == 0, done.stderr
    assert "python-builder 1.0.0 [small/dust_storm]: GoalsComplete" in done.stdout
    record = arcadiabench.score(log)["scorecards"][0]
    assert record["scenario_id"] == "landing-pad-small-dust_storm"
    assert record["completion"]["completed"] == 1
    usage = subprocess.run([sys.executable, "-m", "arcadiabench", "fly"], capture_output=True,
                           text=True, cwd=tmp_path)
    assert usage.returncode == 2 and "usage" in usage.stderr


def test_bench_scores_a_batch_whatever_the_jobs(builder_argv, tmp_path):
    one, many = tmp_path / "one", tmp_path / "many"
    a = arcadiabench.bench(builder_argv, "1-3", jobs=1, out=one, save_logs=True)
    b = arcadiabench.bench(builder_argv, [3, 2, 1], jobs=3, out=many, save_logs=True)
    assert a == b
    card = a["scorecard"]
    assert card["seeds"] == [1, 2, 3]
    assert card["completion"]["success_rate"] == 1.0
    assert card["headline"] == pytest.approx(sum(r["score"] for r in a["runs"]) / 3)
    files = sorted(p.relative_to(one) for p in one.rglob("*") if p.is_file())
    assert [str(f) for f in files[:2]] == ["logs/landing-pad-small-nominal-1.jsonl.gz",
                                         "logs/landing-pad-small-nominal-2.jsonl.gz"]
    for f in files:
        assert (one / f).read_bytes() == (many / f).read_bytes(), f
    assert json.loads((one / "scorecard.json").read_text()) == card
    # Saved logs score to the same scorecard.
    assert arcadiabench.score([one])["scorecards"] == [card]


def test_bench_runs_in_process_coordinators_one_after_another(builder_module):
    fresh = arcadiabench.bench(builder_module.Builder, seeds=[4, 5])
    reused = arcadiabench.bench(builder_module.Builder(), seeds="4-5")
    assert fresh == reused
    assert fresh["scorecard"]["completion"]["completed"] == 2


def test_suite_reports_resilience(builder_argv, tmp_path):
    result = arcadiabench.suite(builder_argv, seeds=1, presets=["small"],
                                variants=["nominal", *arcadiabench.STRESS_VARIANTS], out=tmp_path)
    report = result["report"]
    assert [c["variant"] for c in report["scorecards"]] == ["nominal", *arcadiabench.STRESS_VARIANTS]
    [res] = report["resilience"]
    small = res["presets"][0]
    assert small["preset"] == "small" and small["resilience"] is not None
    assert 0 <= res["resilience"] <= 1
    assert json.loads((tmp_path / "suite.json").read_text()) == report
    with pytest.raises(TypeError):
        arcadiabench.suite(Coordinator())


class Raises(Coordinator):
    def decide(self, observation):
        if observation["seq"] == 2:
            raise KeyError("no such robot")
        return self.decision(observation)


class Teleports(Coordinator):
    def decide(self, observation):
        command = {"command_id": "c1", "robot_id": "ex-1", "type": "teleport"}
        return self.decision(observation, commands=[command])


def test_a_failing_coordinator_raises_with_its_run_and_log(tmp_path):
    log = tmp_path / "fail.jsonl"
    with pytest.raises(CoordinatorError) as caught:
        arcadiabench.run(Raises(), seed=1, log_path=log)
    assert isinstance(caught.value.__cause__, KeyError)
    assert caught.value.result["reason"] == "coordinator_error"
    assert caught.value.result["metrics"]["decisions"] == 2
    assert json.loads(log.read_text().splitlines()[-1])["reason"] == "coordinator_error"

    with pytest.raises(CoordinatorError, match="teleport"):
        arcadiabench.run(Teleports(), seed=1)

    class NoWelcome:
        def welcome(self, hello):
            raise RuntimeError("cannot start")

        def decide(self, observation):
            return {}

    with pytest.raises(CoordinatorError) as caught:
        arcadiabench.run(NoWelcome())
    assert caught.value.result is None
    assert isinstance(caught.value.__cause__, RuntimeError)

    with pytest.raises(TypeError, match="welcome"):
        arcadiabench.run(object())
    with pytest.raises(CoordinatorError, match="could not launch"):
        arcadiabench.run(["/nonexistent/coordinator"])
    # In a batch, a coordinator that fails mid-run is recorded, not raised.
    batch = arcadiabench.bench(Raises, seeds=[1])
    assert batch["runs"][0]["reason"] == "coordinator_error"
    assert "KeyError" in batch["runs"][0]["error"]
    assert batch["scorecard"]["headline"] == 0.0


def test_serve_speaks_ndjson_on_any_streams():
    hello = arcadiabench.scenario(1)
    inbox = io.StringIO("\n".join(json.dumps(m) for m in [
        hello,
        {"type": "observation", "seq": 0, "t": 0.0, "events": [], "robots": []},
        {"type": "end", "t": 1.0, "reason": "time_limit"},
    ]) + "\n")
    outbox = io.StringIO()
    ended = []

    class Echo(Coordinator):
        name = "echo"

        def end(self, end):
            ended.append(end["reason"])

    assert arcadiabench.serve(Echo(), inbox, outbox) == 0
    welcome, decision = [json.loads(line) for line in outbox.getvalue().splitlines()]
    assert welcome == {"type": "welcome", "protocol_version": arcadiabench.PROTOCOL_VERSION,
                       "coordinator": {"name": "echo", "version": "0.0.0"}}
    assert decision == {"type": "decision", "seq": 0, "commands": [], "to_earth": []}
    assert ended == ["time_limit"]
    # The input closing before `end` is a failure.
    assert arcadiabench.serve(Echo(), io.StringIO(json.dumps(hello) + "\n"), io.StringIO()) == 1
