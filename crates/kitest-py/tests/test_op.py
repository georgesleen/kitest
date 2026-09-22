import kitest
from netlists import load
import pytest

# The first line of a SPICE deck is the title and is ignored, so lead with one.
DIVIDER = load("divider.cir")


def test_run_op_solves_divider():
    ng = kitest.Ngspice()
    op = ng.run_op(DIVIDER, [kitest.DcSupply("vin", 5.0)])
    assert op.node("vout").near(2.5, kitest.Tolerance.percent(1.0))
    assert op.node("vout").volts() == pytest.approx(2.5, abs=1e-6)


def test_missing_node_raises():
    ng = kitest.Ngspice()
    op = ng.run_op(DIVIDER, [kitest.DcSupply("vin", 5.0)])
    with pytest.raises(KeyError, match="known: .*vout"):
        op.node("nope")
