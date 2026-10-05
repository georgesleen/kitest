from pathlib import Path

import pytest

import kitest

EXAMPLES = Path(__file__).resolve().parents[3] / "examples" / "kicad"
DIVIDER_SCH = EXAMPLES / "divider" / "divider.kicad_sch"
COLPITTS_SCH = EXAMPLES / "colpitts" / "colpitts.kicad_sch"


def test_divider_from_kicad():
    design = kitest.export_design(str(DIVIDER_SCH))
    netlist = design.netlist()
    ng = kitest.Ngspice()
    op = ng.run_op(netlist.text(), [kitest.DcSupply("+5V", 5.0)])
    assert op.node("/out").near(2.5, kitest.Tolerance.percent(1.0))


def test_colpitts_probe_by_name():
    design = kitest.export_design(str(COLPITTS_SCH))
    probe = design.probe("COLPITTS_OUT")
    assert probe.reference() == "PRB1"
    assert probe.net() == "/OUT"
    assert probe.expect() is None


def test_unknown_probe_lists_known_names():
    design = kitest.export_design(str(COLPITTS_SCH))
    with pytest.raises(RuntimeError, match='probes are "COLPITTS_OUT"'):
        design.probe("OSC")
