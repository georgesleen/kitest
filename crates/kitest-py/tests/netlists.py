"""Load the shared netlists from examples/spice.

The Rust tests pull the same files in with include_str!, so both
languages exercise one artifact per circuit instead of two copies that
drift apart.
"""

from pathlib import Path

SPICE = Path(__file__).resolve().parents[3] / "examples" / "spice"


def load(name: str) -> str:
    """Return the text of examples/spice/<name>."""
    return (SPICE / name).read_text()
