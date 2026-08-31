use kitest::{Ac, AcSupply, Backend, DcSupply, Ngspice, Pulse, Sweep, Tolerance, Tran, TranSource};

const DIVIDER: &str = "\
* voltage divider
r1 vin vout 10k
r2 vout 0 10k
";

const RC: &str = "\
* rc charge
r1 vin vout 1k
c1 vout 0 1u
";

const RC_LOWPASS: &str = "\
* rc low-pass
r1 vin vout 1k
c1 vout 0 1u
";

#[test]
fn op_solves_voltage_divider() {
    let result = Ngspice::default()
        .run_op(DIVIDER, &[DcSupply::new("vin", 5.0)])
        .unwrap();
    let vout = result.node("vout").expect("v(vout) present");
    assert!((vout - 2.5).abs() < 1e-6, "v(vout) = {vout}");
}

#[test]
fn tran_charges_rc() {
    let r = Ngspice::default()
        .run_tran(
            RC,
            &[TranSource::pulse("vin", Pulse::step(0.0, 1.0))],
            Tran {
                step: 1e-5,
                stop: 5e-3,
            },
        )
        .unwrap();
    let vout = r.node("vout").expect("vout present");
    assert!(
        vout.settles_to(0.993, Tolerance::abs(0.02), 1e-3),
        "did not settle"
    );
    assert!(vout.overshoot(1.0) < 0.01, "unexpected overshoot");
}

#[test]
fn ac_rc_lowpass_cutoff() {
    let r = Ngspice::default()
        .run_ac(
            RC_LOWPASS,
            &[AcSupply::new("vin").magnitude(1.0).bias(0.0)],
            Ac {
                sweep: Sweep::Dec,
                points: 100,
                fstart: 1.0,
                fstop: 1e6,
            },
        )
        .unwrap();
    let resp = r.node("vout").expect("vout present");
    let fc = 1.0 / (2.0 * std::f64::consts::PI * 1e3 * 1e-6);
    assert!(
        (resp.gain_db_at(fc).unwrap() + 3.01).abs() < 0.2,
        "gain at cutoff = {}",
        resp.gain_db_at(fc).unwrap()
    );
    assert!(
        (resp.phase_deg_at(fc).unwrap() + 45.0).abs() < 2.0,
        "phase at cutoff = {}",
        resp.phase_deg_at(fc).unwrap()
    );
}
