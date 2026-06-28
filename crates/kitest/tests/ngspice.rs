use kitest::{Analysis, Backend, Ngspice};

const DIVIDER: &str = "\
* voltage divider
v1 vin 0 dc 5
r1 vin vout 10k
r2 vout 0 10k
";

const RC: &str = "\
* rc charge
v1 vin 0 pulse(0 1 0 1n 1n 1 2)
r1 vin vout 1k
c1 vout 0 1u
";

#[test]
fn op_solves_voltage_divider() {
    let r = Ngspice::default().run(DIVIDER, Analysis::Op).unwrap();
    let vout = r.signal("v(vout)").expect("v(vout) present")[0];
    assert!((vout - 2.5).abs() < 1e-6, "v(vout) = {vout}");
}

#[test]
fn tran_charges_rc() {
    let analysis = Analysis::Tran {
        step: 1e-5,
        stop: 5e-3,
    };
    let r = Ngspice::default().run(RC, analysis).unwrap();
    let vout = r.signal("v(vout)").expect("v(vout) present");
    assert!(vout[0] < 0.05, "starts near 0: {}", vout[0]);
    let last = *vout.last().unwrap();
    assert!((last - 0.993).abs() < 0.02, "settles near 1-e^-5: {last}");
}
