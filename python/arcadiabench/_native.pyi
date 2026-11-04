"""Type stubs for the native module. Use the wrappers in ``arcadiabench``."""

from typing import Any, Dict, List, Optional

PROTOCOL_VERSION: str
VERSION: str
PRESETS: List[str]
VARIANTS: List[str]
STRESS_VARIANTS: List[str]
SCORECARD_SCHEMA: str

class CoordinatorError(RuntimeError):
    """A coordinator could not start a run, or failed, timed out or broke the
    protocol during one. When the run got under way, ``result`` holds its run
    record."""

    result: Optional[Dict[str, Any]]

def run_process(
    argv: List[str],
    seed: int,
    preset: str,
    variant: str,
    log_path: Optional[str],
    decision_timeout: float,
) -> Dict[str, Any]: ...
def run_agent(
    agent: Any,
    seed: int,
    preset: str,
    variant: str,
    log_path: Optional[str],
    decision_timeout: float,
) -> Dict[str, Any]: ...
def bench_process(
    argv: List[str],
    seeds: List[int],
    preset: str,
    variant: str,
    jobs: int,
    out: Optional[str],
    save_logs: bool,
    decision_timeout: float,
) -> Dict[str, Any]: ...
def bench_agent(
    factory: Any,
    seeds: List[int],
    preset: str,
    variant: str,
    out: Optional[str],
    save_logs: bool,
    decision_timeout: float,
) -> Dict[str, Any]: ...
def suite_process(
    argv: List[str],
    seeds: List[int],
    presets: List[str],
    variants: List[str],
    jobs: int,
    out: Optional[str],
    save_logs: bool,
    decision_timeout: float,
) -> Dict[str, Any]: ...
def score(paths: List[str]) -> Dict[str, Any]: ...
def scenario(seed: int, preset: str, variant: str) -> Dict[str, Any]: ...
def parse_seeds(spec: str) -> List[int]: ...
def cli(args: List[str]) -> int: ...
