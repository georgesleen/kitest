//! The walking skeleton: a KiCad design driven through the full pipeline.

use kitest::{Backend, DcSupply, Ngspice, Tolerance, export_netlist};
use std::path::Path;

const DIVIDER_SCH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kicad/divider/divider.kicad_sch"
);

#[test]
fn divider_op_from_kicad() {
    let netlist = export_netlist(Path::new(DIVIDER_SCH)).unwrap();

    let result = Ngspice::default()
        .run_op(&netlist, &[DcSupply::new("+5V", 5.0)])
        .unwrap();
    let out = result.node("/out").expect("/out present");

    assert!(out.near(2.5, Tolerance::abs(1e-6)), "out = {}", out.volts());
}
