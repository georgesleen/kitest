use kitest::{
    Backend, CheckError, Config, ModelLibrary, Ngspice, SupplyProblem, Tran,
    check_project, export_design, export_netlist,
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

/// The colpitts design with probe PRB1's Expect field set to `expect`.
fn colpitts_expecting(expect: &str) -> kitest::Design {
    let mut design =
        export_design(Path::new(&format!("{COLPITTS}/colpitts.kicad_sch")))
            .unwrap();
    let probe = design
        .components
        .iter_mut()
        .find(|component| component.reference == "PRB1")
        .expect("PRB1 present");
    probe.fields.insert("Expect".into(), expect.into());
    design
}

#[test]
fn the_colpitts_probe_passes_its_oscillation_check() {
    let design =
        colpitts_expecting("oscillates(near=10.115e6, within=percent(2))");
    let config = Config::for_project(Path::new(COLPITTS)).expect("config");
    let report = design.check(&config, &Ngspice::default()).expect("ran");

    assert_eq!(report.outcomes.len(), 1);
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.probe, "COLPITTS_OUT");
    assert_eq!(outcome.corner, [("VCC".to_owned(), 9.0)]);
    let check = outcome.check.as_ref().expect("a check ran");
    assert!(check.passed(), "{check}");
    assert!(report.passed());
}

#[test]
fn the_colpitts_probe_fails_a_wrong_frequency_and_says_what_it_measured() {
    let design = colpitts_expecting("oscillates(near=12e6, within=percent(2))");
    let config = Config::for_project(Path::new(COLPITTS)).expect("config");
    let report = design.check(&config, &Ngspice::default()).expect("ran");

    let check = report.outcomes[0].check.as_ref().expect("a check ran");
    assert!(!check.passed());
    assert!(check.message().contains("MHz from 12 MHz"), "{check}");
    assert!(!report.passed());
}

#[test]
fn a_supply_reached_through_an_undeclared_label_is_named_in_the_failure() {
    // VCC as a local label, not a power symbol, so nothing resolves it.
    let mut design = colpitts_expecting("oscillates(near=10.115e6, within=2%)");
    design.rails.retain(|rail| rail != "VCC");
    let net = design
        .nets
        .iter_mut()
        .find(|net| net.name == "VCC")
        .unwrap();
    net.name = "VSUP".into();
    for component in &mut design.components {
        for net in component.pins.values_mut() {
            if net == "VCC" {
                *net = "VSUP".into();
            }
        }
    }
    let report = design
        .check(&Config::default(), &Ngspice::default())
        .unwrap();

    let outcome = &report.outcomes[0];
    assert!(!outcome.passed());
    let undriven = outcome
        .diagnosis
        .iter()
        .find(|finding| finding.starts_with("VSUP sits at 0 V"));
    assert!(
        undriven.is_some_and(|finding| finding.contains("Q1's collector")),
        "{:?}",
        outcome.diagnosis
    );
}

#[test]
fn the_colpitts_probe_checks_its_dc_bias() {
    let design = colpitts_expecting("dc(near=3.7, within=abs(0.1))");
    let config = Config::for_project(Path::new(COLPITTS)).expect("config");
    let report = design.check(&config, &Ngspice::default()).expect("ran");

    let check = report.outcomes[0].check.as_ref().expect("a check ran");
    assert!(check.passed(), "{check}");
}

#[test]
fn an_unreadable_expect_names_the_probe_before_simulating() {
    let design = colpitts_expecting("oscilates(near=1e7, within=percent(2))");
    let config = Config::for_project(Path::new(COLPITTS)).expect("config");
    let error = design.check(&config, &Ngspice::default()).unwrap_err();

    assert!(matches!(error, CheckError::Expect { .. }), "{error}");
    let message = error.to_string();
    assert!(message.contains("PRB1 Expect"), "{message}");
    assert!(message.contains("\"oscilates\""), "{message}");
}

#[test]
fn checking_the_project_directory_finds_its_schematic() {
    let report = check_project(Path::new(COLPITTS)).expect("ran");
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].check, None);
    assert!(report.passed());
}
