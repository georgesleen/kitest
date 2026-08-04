use kitest::export_netlist;
use std::path::Path;

const DIVIDER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/divider");

#[test]
fn exports_divider_matching_golden() {
    let netlist = export_netlist(Path::new(&format!("{DIVIDER}/divider.kicad_sch"))).unwrap();

    let golden = std::fs::read_to_string(format!("{DIVIDER}/divider.spice")).unwrap();
    let expected = golden.trim_end().trim_end_matches(".end").trim_end();

    assert_eq!(netlist, expected);
}
