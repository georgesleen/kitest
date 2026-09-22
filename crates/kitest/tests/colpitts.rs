//! Spike: does ngspice start a Colpitts, and can kitest measure it?

use std::f64::consts::PI;

use kitest::{Backend, Ngspice, Tran, TranSource};

/// Stands in for a KiCad export until the schematic exists.
const COLPITTS: &str = include_str!("../../../examples/spice/colpitts.cir");

const INDUCTANCE: f64 = 1e-6;
const TANK_CAP: f64 = 470e-12;

/// 1 / (2 pi sqrt(L C)) for the two tank capacitors in series.
fn tank_hertz() -> f64 {
    let series = TANK_CAP * TANK_CAP / (TANK_CAP + TANK_CAP);
    1.0 / (2.0 * PI * (INDUCTANCE * series).sqrt())
}

#[test]
fn colpitts_starts_and_runs_at_the_tank_frequency() {
    let r = Ngspice::default()
        .run_tran(
            COLPITTS,
            // Supply noise starts it, the way a real one starts.
            &[TranSource::dc("vcc", 9.0)],
            Tran::new(1e-9, 25e-6).start(5e-6),
        )
        .expect("simulation ran");

    let tone = r.node("base").expect("base present").dominant_tone();

    // It has to have started at all before the frequency means
    // anything: a dead oscillator still reports some number.
    let volts = tone.amplitude().volts();
    assert!(volts > 1.0, "did not start, amplitude = {volts}");

    // And the run has to be fine enough to believe.
    let per_cycle = tone.samples_per_cycle();
    assert!(per_cycle > 20.0, "too coarse, {per_cycle} per cycle");

    // 5%, not 1%: the ideal LC formula ignores the transistor's own
    // junction capacitance, which sits across the tank and pulls the
    // frequency down by a couple of percent. Measured 10.15 against
    // 10.38 MHz nominal.
    let hertz = tone.frequency().hertz();
    let error = (hertz - tank_hertz()).abs() / tank_hertz();
    assert!(error < 0.05, "frequency = {hertz}");
}
