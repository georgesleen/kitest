import kitest
from netlists import load

RC = load("rc.cir")


def test_run_tran_charges_rc():
    ng = kitest.Ngspice()
    wf = ng.run_tran(
        RC,
        [kitest.TranSource.pulse("vin", kitest.Pulse.step(0.0, 1.0))],
        kitest.Tran(1e-5, 5e-3),
    )
    vout = wf.node("vout")
    assert vout.settles_to(0.993, kitest.Tolerance.abs(0.02), 1e-3)
    assert vout.overshoot(1.0) < 0.01
