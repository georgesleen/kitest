import math

import kitest

RC_LOWPASS = "* rc low-pass\nr1 vin vout 1k\nc1 vout 0 1u"


def test_run_ac_lowpass_cutoff():
    ng = kitest.Ngspice()
    sp = ng.run_ac(
        RC_LOWPASS,
        [kitest.AcSupply("vin")],
        kitest.Ac(kitest.Sweep.Dec, 100, 1.0, 1e6),
    )
    resp = sp.node("vout")
    fc = 1.0 / (2.0 * math.pi * 1e3 * 1e-6)
    assert abs(resp.gain_db_at(fc) + 3.01) < 0.2
    assert abs(resp.phase_deg_at(fc) + 45.0) < 2.0
