from pathlib import Path

import kitest

DIVIDER_SCH = (
    Path(__file__).resolve().parents[3]
    / "examples"
    / "divider"
    / "divider.kicad_sch"
)


def test_divider_from_kicad():
    netlist = kitest.export_netlist(str(DIVIDER_SCH))
    ng = kitest.Ngspice()
    op = ng.run_op(netlist, [kitest.DcSupply("+5V", 5.0)])
    assert op.node("/out").near(2.5, kitest.Tolerance.percent(1.0))
