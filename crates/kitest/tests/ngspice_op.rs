use kitest::{Analysis, Backend, Ngspice};

const DIVIDER: &str = "\
* voltage divider
v1 vin 0 dc 5
r1 vin vout 10k
r2 vout 0 10k
";

#[test]
fn op_solves_voltage_divider() {
    let r = Ngspice::default().run(DIVIDER, Analysis::Op).unwrap();
    let vout = r.signal("v(vout)").expect("v(vout) present")[0];
    assert!((vout - 2.5).abs() < 1e-6, "v(vout) = {vout}");
}
