import importlib.util
import sys
from pathlib import Path

import pytest

EXAMPLES = Path(__file__).resolve().parent.parent / "examples"


@pytest.fixture(scope="session")
def builder_module():
    """The example coordinator, imported from python/examples."""
    spec = importlib.util.spec_from_file_location("builder_example", EXAMPLES / "builder.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


@pytest.fixture(scope="session")
def builder_argv():
    """The example coordinator as a subprocess speaking NDJSON through serve()."""
    return [sys.executable, str(EXAMPLES / "builder.py"), "--serve"]
