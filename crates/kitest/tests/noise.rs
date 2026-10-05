//! Rail noise as an excitation, and how far it gets.
//!
//! A passive tank has no gain, so an ideal rail leaves it dead and a
//! noisy one rings it. Decoupling puts a low-pass between the two.

use std::f64::consts::PI;

use kitest::{Backend, Ngspice, Signal, Tran, TranSource, Transient};

const TANK: &str = include_str!("../../../examples/spice/tank.cir");
const DECOUPLED: &str =
    include_str!("../../../examples/spice/tank-decoupled.cir");

const INDUCTANCE: f64 = 1e-6;
const CAPACITANCE: f64 = 235e-12;

/// The resonance the tank should ring at.
fn resonant_hertz() -> f64 {
    1.0 / (2.0 * PI * (INDUCTANCE * CAPACITANCE).sqrt())
}

/// A rail with enough noise to excite the tank.
fn noisy_rail() -> TranSource {
    TranSource::noisy_dc("vcc", 9.0, 1e-3)
}

fn run(netlist: &str, rail: TranSource) -> Transient {
    Ngspice::default()
        .run_tran(netlist, &[rail], Tran::new(1e-9, 20e-6).start(10e-6))
        .expect("simulation ran")
}

fn tank_of(result: &Transient) -> Signal<'_> {
    result.node("tank").expect("tank present")
}

#[test]
fn a_noisy_rail_rings_a_passive_tank() {
    let result = run(TANK, noisy_rail());
    let tone = tank_of(&result).dominant_tone();

    let volts = tone.amplitude().volts();
    assert!(volts > 1e-6, "amplitude = {volts}");

    // 6%: a noise-driven peak lands anywhere across the 2.6% wide
    // resonance (loaded Q near 38); measured -2.4% to +4.8% over 40 runs.
    let hertz = tone.frequency().hertz();
    let error = (hertz - resonant_hertz()).abs() / resonant_hertz();
    assert!(error < 0.06, "frequency = {hertz}");
}

#[test]
fn an_ideal_rail_leaves_a_passive_tank_dead() {
    let result = run(TANK, TranSource::dc("vcc", 9.0));
    let volts = tank_of(&result).dominant_tone().amplitude().volts();
    assert!(volts < 1e-12, "amplitude = {volts}");
}

#[test]
fn decoupling_the_rail_starves_the_tank() {
    // Good power integrity works against noise-driven startup: the
    // supply impedance and the decoupling capacitor form a low-pass
    // that keeps the noise away from the circuit it would have
    // started. Measured about 100x down.
    let bare = run(TANK, noisy_rail());
    let bare_volts = tank_of(&bare).dominant_tone().amplitude().volts();

    let quiet = run(DECOUPLED, noisy_rail());
    let quiet_volts = tank_of(&quiet).dominant_tone().amplitude().volts();

    let ratio = bare_volts / quiet_volts;
    assert!(
        ratio > 10.0,
        "only {ratio}x down: {bare_volts} {quiet_volts}"
    );
}
