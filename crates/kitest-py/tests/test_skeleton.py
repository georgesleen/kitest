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
    with pytest.raises(kitest.ProbeError, match='probes are "COLPITTS_OUT"'):
        design.probe("OSC")


def test_colpitts_oscillates_from_its_project_config():
    design = kitest.export_design(str(COLPITTS_SCH))
    config = kitest.Config.for_project(str(COLPITTS_SCH.parent))
    netlist = design.netlist(config.model_libraries())
    assert netlist.defaulted() == []
    corners = design.power(config.supplies()).corners()
    assert [corner.voltages() for corner in corners] == [[("VCC", 9.0)]]

    tone = (
        kitest.Ngspice()
        .run_tran(
            netlist.text(),
            corners[0].tran_sources(),
            kitest.Tran(1e-9, 25e-6).start(10e-6),
        )
        .node("/OUT")
        .dominant_tone()
    )
    assert tone.frequency().near(10.115e6, kitest.Tolerance.percent(1.0))
    assert tone.amplitude().volts() > 1.0


def test_colpitts_rails_say_how_each_is_powered():
    design = kitest.export_design(str(COLPITTS_SCH))
    power = design.power({"VCC": [5.0, 9.0]})
    rails = {rail.net(): rail for rail in power.rails()}
    assert rails["GND"].kind() == "ground"
    assert rails["VCC"].kind() == "source"
    assert rails["VCC"].voltages() == [5.0, 9.0]
    assert rails["VCC"].origin() == "declared"


def test_colpitts_without_vcc_says_what_to_add():
    design = kitest.export_design(str(COLPITTS_SCH))
    with pytest.raises(kitest.SupplyError, match='"VCC" = <volts>'):
        design.power()


def test_bad_config_reports_where_toml_broke(tmp_path):
    (tmp_path / "kitest.toml").write_text("[supplies]\nVCC = \n")
    with pytest.raises(kitest.ConfigError) as error:
        kitest.Config.for_project(str(tmp_path))
    message = str(error.value)
    assert "is not a valid kitest config" in message
    assert "line 2" in message
