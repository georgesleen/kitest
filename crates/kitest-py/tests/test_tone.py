import kitest

# A sine drives a divider, so vout follows vin at half the amplitude.
DIVIDER = "* tone divider\nr1 vin vout 1k\nr2 vout 0 1k"

HERTZ = 1000.0
AMPLITUDE = 2.0
BIAS = 5.0


def _tone():
    ng = kitest.Ngspice()
    wf = ng.run_tran(
        DIVIDER,
        [kitest.TranSource.sin("vin", kitest.Sin(BIAS, AMPLITUDE, HERTZ))],
        kitest.Tran(1e-5, 20e-3),
    )
    return wf.node("vout").dominant_tone()


def test_dominant_tone_measures_the_drive_frequency():
    assert _tone().frequency().near(HERTZ, kitest.Tolerance.percent(1.0))


def test_dominant_tone_measures_the_divided_amplitude():
    # The divider halves the drive, and the bias must not disturb it.
    assert _tone().amplitude().near(
        AMPLITUDE / 2.0, kitest.Tolerance.percent(5.0)
    )
