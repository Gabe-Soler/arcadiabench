"""ArcadiaBench for Python: run, benchmark and score coordinators for robot
crews building a landing pad at Arcadia Planitia, Mars.

- :func:`run` runs one seed of a scenario with a coordinator.
- :func:`bench` runs a coordinator over many seeds and scores the batch.
- :func:`suite` does that across presets and variants, with resilience.
- :func:`score` builds scorecards from saved event logs.
- :func:`scenario` gives the ``hello`` a coordinator gets.
- :class:`Coordinator` and :func:`serve` help write coordinators in Python.

A coordinator is either a command line (a list of strings, or one path),
which runs as a subprocess speaking newline-delimited JSON, or a Python
object with ``welcome(hello)``, ``decide(observation)`` and ``end(end)``,
which runs in process. Either way, every message it sends is checked
against arcadia-protocol's JSON Schemas.

Scenarios are named by a preset (``small``, ``medium``, ``large``) and a
variant (``nominal``, ``dust_storm``, ``conjunction``, ``cascading_faults``,
``solar_only``, ``nuclear_heavy``), in a catalog (:data:`CATALOGS`). A
catalog fixes the robots, the crews and every parameter, so results on it
stay comparable; ``catalog=None`` means :data:`DEFAULT_CATALOG`, the newest.

The fleet design track swaps the preset for a crew of your own: pass
``crew=`` a mapping of design to count, such as ``{"scout": 1, "excavator":
2, "hauler": 1, "compactor_sinterer": 6}``, or the path of a JSON file
holding one. A crew has 4 to 20 robots and weighs no more than the catalog's
budget (:data:`MASS_BUDGETS_KG`); otherwise ``ValueError`` is raised.
"""

from __future__ import annotations

import json
import os
from typing import Any, Callable, Dict, Iterable, List, Mapping, Optional, Sequence, Union

from . import _native
from ._native import (
    CATALOGS,
    CREW_SCHEMA,
    DEFAULT_CATALOG,
    MASS_BUDGETS_KG,
    PRESETS,
    PROTOCOL_VERSION,
    SCORECARD_SCHEMA,
    STRESS_VARIANTS,
    VARIANTS,
    CoordinatorError,
)
from .coordinator import Coordinator, serve

__version__: str = _native.VERSION

__all__ = [
    "CATALOGS",
    "CREW_SCHEMA",
    "DEFAULT_CATALOG",
    "MASS_BUDGETS_KG",
    "PRESETS",
    "PROTOCOL_VERSION",
    "SCORECARD_SCHEMA",
    "STRESS_VARIANTS",
    "VARIANTS",
    "Coordinator",
    "CoordinatorError",
    "bench",
    "run",
    "scenario",
    "score",
    "serve",
    "suite",
    "__version__",
]

PathLike = Union[str, "os.PathLike[str]"]
Seeds = Union[str, int, Iterable[int]]
Crew = Union[Mapping[str, int], PathLike]


def _crew(crew: Optional[Crew]) -> Optional[str]:
    """A crew as the JSON text of a crew file, or None for a preset."""
    if crew is None:
        return None
    if isinstance(crew, (str, os.PathLike)):
        with open(crew, encoding="utf-8") as f:
            return f.read()
    if isinstance(crew, Mapping):
        return json.dumps(dict(crew))
    raise TypeError(
        f"a crew is a mapping of design to count or the path of a crew file, "
        f"not {type(crew).__name__}"
    )


def _argv(coordinator: Any) -> Optional[List[str]]:
    """The command line of a subprocess coordinator, or None for an object."""
    if isinstance(coordinator, (str, os.PathLike)):
        return [os.fspath(coordinator)]
    if isinstance(coordinator, (list, tuple)):
        if not coordinator:
            raise ValueError("empty coordinator command")
        return [os.fspath(part) for part in coordinator]
    return None


def _check_agent(agent: Any) -> Any:
    for method in ("welcome", "decide"):
        if not callable(getattr(agent, method, None)):
            raise TypeError(
                f"a coordinator is a command line or an object with welcome() and decide(); "
                f"{type(agent).__name__} has no {method}()"
            )
    return agent


def _path(path: Optional[PathLike]) -> Optional[str]:
    return None if path is None else os.fspath(path)


def _seeds(seeds: Seeds) -> List[int]:
    if isinstance(seeds, str):
        return _native.parse_seeds(seeds)
    if isinstance(seeds, int):
        return [seeds]
    out = sorted(int(s) for s in seeds)
    if not out:
        raise ValueError("no seeds")
    for a, b in zip(out, out[1:]):
        if a == b:
            raise ValueError(f"seed {a} is listed twice")
    return out


def run(
    coordinator: Any,
    seed: int = 0,
    preset: str = "small",
    variant: str = "nominal",
    log_path: Optional[PathLike] = None,
    decision_timeout: float = 10.0,
    *,
    catalog: Optional[str] = None,
    crew: Optional[Crew] = None,
) -> Dict[str, Any]:
    """Run one seed of a scenario with a coordinator: the catalog's
    ``landing-pad-<preset>-<variant>`` (a catalog after 1.0 puts its name
    before the preset, as in ``landing-pad-1.1-small-nominal``). With
    ``crew``, the crew takes the preset's place (the fleet design track).

    ``coordinator`` is a command line (a list of strings, or one path), run
    as a subprocess, or an object with ``welcome``, ``decide`` and ``end``,
    run in process. ``log_path`` saves the run's event log (gzipped if it
    ends in ``.gz``). Each decision must come within ``decision_timeout``
    seconds; an in-process coordinator is never interrupted, but a late
    reply ends the run.

    Returns the run record (as in a batch's ``metrics.jsonl``): its id,
    scenario, seed, coordinator, end reason, speed, score and ``metrics``
    (``run_end.metrics``), plus the session report's ``end_t`` (s),
    ``progress`` (share of the pad in spec) and ``lost_link_returns``. Raises
    :class:`CoordinatorError` if the coordinator fails, times out or breaks
    the protocol; when the run got under way, the error's ``result`` holds
    its record and the log is complete. An exception raised by an in-process
    coordinator is the error's cause.
    """
    argv = _argv(coordinator)
    if argv is not None:
        return _native.run_process(
            argv, seed, catalog, _crew(crew), preset, variant, _path(log_path), decision_timeout
        )
    return _native.run_agent(
        _check_agent(coordinator),
        seed,
        catalog,
        _crew(crew),
        preset,
        variant,
        _path(log_path),
        decision_timeout,
    )


def bench(
    coordinator: Any,
    seeds: Seeds = "1-50",
    preset: str = "small",
    variant: str = "nominal",
    *,
    catalog: Optional[str] = None,
    crew: Optional[Crew] = None,
    jobs: Optional[int] = None,
    out: Optional[PathLike] = None,
    save_logs: bool = False,
    decision_timeout: float = 10.0,
) -> Dict[str, Any]:
    """Run a coordinator over seeds of one scenario and score the batch.
    With ``crew``, the crew takes the preset's place.

    ``seeds`` is a list such as ``"1-50"`` or ``"1,4,10-20"``, an int, or
    any iterable of ints. A command-line coordinator runs ``jobs`` seeds at
    once (default: the number of CPUs), each in a fresh process. An
    in-process coordinator runs seeds one after another: pass a class to get
    a fresh instance for each seed, or an object to reuse it (each run
    starts with its ``welcome``).

    With ``out``, writes ``metrics.jsonl`` and ``scorecard.json`` there, and
    with ``save_logs`` every event log under ``out/logs``. The results never
    depend on ``jobs``.

    Returns ``{"scorecard": ..., "runs": [...]}``. A coordinator that fails
    mid-run is recorded in its run, not raised; one that cannot start a run
    raises :class:`CoordinatorError`.
    """
    seed_list = _seeds(seeds)
    argv = _argv(coordinator)
    if argv is not None:
        return _native.bench_process(
            argv,
            seed_list,
            catalog,
            _crew(crew),
            preset,
            variant,
            jobs or 0,
            _path(out),
            save_logs,
            decision_timeout,
        )
    factory: Callable[[], Any]
    if isinstance(coordinator, type):
        factory = coordinator
    else:
        agent = _check_agent(coordinator)

        def reuse() -> Any:
            return agent

        factory = reuse
    return _native.bench_agent(
        factory,
        seed_list,
        catalog,
        _crew(crew),
        preset,
        variant,
        _path(out),
        save_logs,
        decision_timeout,
    )


def suite(
    coordinator: Union[Sequence[str], PathLike],
    seeds: Seeds = "1-50",
    presets: Optional[Iterable[str]] = None,
    variants: Optional[Iterable[str]] = None,
    *,
    catalog: Optional[str] = None,
    crew: Optional[Crew] = None,
    jobs: Optional[int] = None,
    out: Optional[PathLike] = None,
    save_logs: bool = False,
    decision_timeout: float = 10.0,
) -> Dict[str, Any]:
    """Run a command-line coordinator on every preset × variant of a
    catalog over seeds, or with ``crew`` on every variant with that crew
    (the fleet design track), whose resilience the report then gives.

    Defaults to every preset and every variant. With ``out``, each scenario's
    results go to ``out/<preset>-<variant>/`` (on a catalog after 1.0,
    ``out/<catalog>-<preset>-<variant>/``) and the report to
    ``out/suite.json``. Returns ``{"report": ..., "runs": [...]}``, where the
    report holds every scorecard and the coordinator's resilience.
    """
    argv = _argv(coordinator)
    if argv is None:
        raise TypeError("suite() runs a command-line coordinator; use bench() for objects")
    if crew is not None and presets is not None:
        raise ValueError("a crew takes the place of the presets: give one or the other")
    return _native.suite_process(
        argv,
        _seeds(seeds),
        catalog,
        _crew(crew),
        list(PRESETS if presets is None else presets),
        list(VARIANTS if variants is None else variants),
        jobs or 0,
        _path(out),
        save_logs,
        decision_timeout,
    )


def score(paths: Union[PathLike, Iterable[PathLike]]) -> Dict[str, Any]:
    """Scorecards and resilience from saved event logs.

    ``paths`` are log files (``.jsonl`` or ``.jsonl.gz``), a bench's
    ``metrics.jsonl`` files, and directories, searched for logs. Returns
    ``{"scorecards": [...], "resilience": [...]}`` with one scorecard per
    coordinator and scenario, and one resilience per coordinator and catalog.
    """
    if isinstance(paths, (str, os.PathLike)):
        paths = [paths]
    return _native.score([os.fspath(p) for p in paths])


def scenario(
    seed: int = 0,
    preset: str = "small",
    variant: str = "nominal",
    *,
    catalog: Optional[str] = None,
    crew: Optional[Crew] = None,
) -> Dict[str, Any]:
    """The ``hello`` message a coordinator gets for this scenario and seed:
    the protocol version, the simulator, the run id, the seed, and the
    ``scenario`` (site prior, crew, stations, depots, power, base station,
    policy, constants)."""
    return _native.scenario(seed, catalog, _crew(crew), preset, variant)
