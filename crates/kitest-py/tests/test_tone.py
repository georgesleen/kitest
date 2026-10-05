import kitest
from netlists import load

# A sine drives a divider, so vout follows vin at half the amplitude.
DIVIDER = load("divider.cir")

HERTZ = 1000.0
AMPLITUDE = 2.0
BIAS = 5.0
SPAN = 20e-3
DELAY = 5e-3


def _tone(tran, delay=0.0):
    ng = kitest.Ngspice()
    drive = kitest.Sin(offset=BIAS, amplitude=AMPLITUDE, freq=HERTZ)
    if delay:
        drive = drive.delay(delay)
    wf = ng.run_tran(
        DIVIDER, [kitest.TranSource.sin("vin", drive)], tran
    )
    return wf.node("vout").dominant_tone()


def test_dominant_tone_measures_the_drive_frequency():
    tone = _tone(kitest.Tran(step=1e-5, stop=SPAN))
    assert tone.frequency().near(HERTZ, kitest.Tolerance.percent(1.0))


def test_dominant_tone_measures_the_divided_amplitude():
    # The divider halves the drive, and the bias must not disturb it.
    tone = _tone(kitest.Tran(step=1e-5, stop=SPAN))
    assert tone.amplitude().near(
        AMPLITUDE / 2.0, kitest.Tolerance.percent(1.0)
    )


def test_discarding_startup_fixes_the_amplitude():
    # The drive starts at 5 ms, so a window from zero averages in the
    # quiet stretch before it and reads about 9% low.
    tight = kitest.Tolerance.percent(1.0)
    whole = _tone(kitest.Tran(step=1e-5, stop=SPAN), delay=DELAY)
    steady = _tone(kitest.Tran(step=1e-5, stop=SPAN).start(10e-3), delay=DELAY)

    assert not whole.amplitude().near(AMPLITUDE / 2.0, tight)
    assert steady.amplitude().near(AMPLITUDE / 2.0, tight)
