use kitest::{
    Backend, Config, ModelLibrary, Ngspice, SupplyProblem, Tran, export_design,
    export_netlist,
};
use std::collections::BTreeMap;
use std::path::Path;

const DIVIDER: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/kicad/divider");
const COLPITTS: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/kicad/colpitts");

/// The element lines of a SPICE deck, sorted.
fn elements(deck: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = deck
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('.'))
        .collect();
    lines.sort_unstable();
    lines
}

#[test]
fn exports_divider_matching_golden() {
    let netlist =
        export_netlist(Path::new(&format!("{DIVIDER}/divider.kicad_sch")))
            .unwrap();

    let golden =
        std::fs::read_to_string(format!("{DIVIDER}/divider.spice")).unwrap();

    assert_eq!(elements(&netlist), elements(&golden));
}

#[test]
fn reads_the_drawn_colpitts_as_a_design() {
    let design =
        export_design(Path::new(&format!("{COLPITTS}/colpitts.kicad_sch")))
            .unwrap();

    assert_eq!(design.rails, ["GND", "VCC"]);
    assert!(
        design
            .components
            .iter()
            .all(|c| !c.reference.starts_with('#')),
        "power symbols are rails, not components"
    );

    let q1 = design.component("Q1").expect("Q1 present");
    assert_eq!(q1.value, "2N3904");
    assert_eq!(q1.pins["1"], "/OUT");
    assert_eq!(q1.pins["3"], "VCC");

    let part = design.part(q1).expect("2N3904 definition present");
    let roles: Vec<_> = ["1", "2", "3"]
        .iter()
        .map(|number| part.pin(number).expect("pin").name.as_str())
        .collect();
    assert_eq!(roles, ["E", "B", "C"]);
    assert_eq!(part.fields["Sim.Device"], "NPN");
    assert_eq!(part.fields["Sim.Pins"], "1=E 2=B 3=C");

    assert!(!design.net("VCC").expect("VCC net").is_driven());
}

#[test]
fn the_drawn_colpitts_oscillates_from_its_own_netlist() {
    let design =
        export_design(Path::new(&format!("{COLPITTS}/colpitts.kicad_sch")))
            .unwrap();
    let config = Config::for_project(Path::new(COLPITTS)).expect("config");
    let netlist = design
        .netlist(&[ModelLibrary::bundled()])
        .expect("models bind");
    assert!(netlist.defaulted.is_empty(), "{:?}", netlist.defaulted);
    let power = design.power(&config.supplies).expect("supplies resolve");
    let corners: Vec<_> = power.corners().collect();
    assert_eq!(corners.len(), 1);

    let tone = Ngspice::default()
        .run_tran(
            &netlist.text,
            &corners[0].tran_sources(),
            Tran::new(1e-9, 25e-6).start(10e-6),
        )
        .expect("simulation ran")
        .node("/OUT")
        .expect("/OUT present")
        .dominant_tone();

    let hertz = tone.frequency().hertz();
    assert!((hertz - 10.115e6).abs() / 10.115e6 < 0.01, "{hertz}");
    let volts = tone.amplitude().volts();
    assert!(volts > 1.0, "did not start, amplitude = {volts}");
}

#[test]
fn the_colpitts_without_a_vcc_voltage_says_what_to_add() {
    let design =
        export_design(Path::new(&format!("{COLPITTS}/colpitts.kicad_sch")))
            .unwrap();
    let error = design.power(&BTreeMap::new()).unwrap_err();

    assert!(matches!(
        error.problems.as_slice(),
        [SupplyProblem::Unresolved { net, .. }] if net == "VCC"
    ));
    let message = error.to_string();
    assert!(message.contains("Q1 (2N3904) pin C"), "{message}");
    assert!(message.contains("\"VCC\" = <volts>"), "{message}");
}

#[test]
fn the_drawn_colpitts_probe_is_read_by_name() {
    let design =
        export_design(Path::new(&format!("{COLPITTS}/colpitts.kicad_sch")))
            .unwrap();
    let probe = design.probe("COLPITTS_OUT").expect("probe read");

    assert_eq!(probe.reference(), "PRB1");
    assert_eq!(probe.net().name, "/OUT");
    assert_eq!(probe.expect(), None);
}
