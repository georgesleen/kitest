//! A current kick rings a resonance in proportion to its size.
//!
//! A passive tank has no mechanism to set its own amplitude, so the
//! kick alone decides how hard it rings. An ideal rail leaves the same
//! tank dead, which is what leaves the kick as the only excitation.

use kitest::{Backend, Ngspice, Tran, TranSource};

const TANK: &str = include_str!("../../../examples/spice/tank.cir");

/// Tank amplitude after kicking it with `amps` on an otherwise silent rail.
fn tank_volts(amps: f64) -> f64 {
    Ngspice::default()
        .run_tran(
            TANK,
            &[TranSource::dc("vcc", 9.0), TranSource::kick("tank", amps)],
            Tran::new(1e-9, 20e-6).start(10e-6),
        )
        .expect("simulation ran")
        .node("tank")
        .expect("tank present")
        .dominant_tone()
        .amplitude()
        .volts()
}

#[test]
fn a_resonance_follows_the_size_of_its_kick() {
    let volts = [tank_volts(1e-9), tank_volts(1e-6), tank_volts(1e-3)];

    for pair in volts.windows(2) {
        let gain = pair[1] / pair[0];
        assert!((gain - 1e3).abs() / 1e3 < 0.05, "{gain}x for 1000x");
    }
}

#[test]
fn a_kick_is_the_only_thing_that_rings_the_tank() {
    let kicked = tank_volts(1e-6);

    let quiet = Ngspice::default()
        .run_tran(
            TANK,
            &[TranSource::dc("vcc", 9.0)],
            Tran::new(1e-9, 20e-6).start(10e-6),
        )
        .expect("simulation ran")
        .node("tank")
        .expect("tank present")
        .dominant_tone()
        .amplitude()
        .volts();

    assert!(kicked / quiet > 100.0, "{kicked} against {quiet}");
}
