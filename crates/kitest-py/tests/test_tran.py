import json

import kitest
from netlists import load

RC = load("rc.cir")


def test_run_tran_charges_rc():
    ng = kitest.Ngspice()
    wf = ng.run_tran(
        RC,
        [kitest.TranSource.pulse("vin", kitest.Pulse.step(low=0.0, high=1.0))],
        kitest.Tran(step=1e-5, stop=5e-3),
    )
    vout = wf.node("vout")
    assert vout.settles_to(0.993, kitest.Tolerance.abs(0.02), window=1e-3)
    assert vout.overshoot(1.0) < 0.01


def test_capture_saves_a_settling_expectation(tmp_path):
    ng = kitest.Ngspice()
    transient = ng.run_tran(
        RC,
        [kitest.TranSource.pulse("vin", kitest.Pulse.step(low=0.0, high=1.0))],
        kitest.Tran(step=1e-5, stop=5e-3),
    )
    check = transient.node("vout").settles_to(
        1.0, kitest.Tolerance.abs(0.02), window=1e-3
    )
    capture = transient.capture("rc-step")
    capture.expect("vout", check)
    path = tmp_path / "rc-step.json"
    capture.save(str(path))

    saved = json.loads(path.read_text())
    assert saved["version"] == 2
    assert saved["name"] == "rc-step"
    assert saved["expectations"] == [
        {
            "trace": "vout",
            "passed": True,
            "message": str(check),
            "region": {
                "kind": "band",
                "start": 0.004,
                "end": 0.005,
                "low": 0.98,
                "high": 1.02,
            },
        }
    ]
