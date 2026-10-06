use kitest::{
    Backend, CheckError, Config, Design, ModelLibrary, Ngspice, PinKind,
    SupplyProblem, Tran, check_project, export_design, export_netlist,
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

/// Every net's pins as `(reference, pin, kind)`, sorted, by net name.
fn connectivity(design: &Design) -> BTreeMap<&str, Vec<(&str, &str, PinKind)>> {
    design
        .nets
        .iter()
        .map(|net| {
            let mut pins: Vec<_> = net
                .nodes
                .iter()
                .map(|node| {
                    (node.reference.as_str(), node.pin.as_str(), node.kind)
                })
                .collect();
            pins.sort_unstable_by_key(|&(reference, pin, _)| (reference, pin));
            (net.name.as_str(), pins)
        })
        .collect()
}

/// Every component as `(reference, value, "library:part")`, sorted.
fn parts_list(design: &Design) -> Vec<(&str, &str, String)> {
    let mut parts: Vec<_> = design
        .components
        .iter()
        .map(|c| {
            let id = format!("{}:{}", c.library.library, c.library.part);
            (c.reference.as_str(), c.value.as_str(), id)
        })
        .collect();
    parts.sort_unstable();
    parts
}

// The two tests below state what the examples are drawn as, written by hand
// from the schematics. Comparing against a recorded export instead would pass
// on a broken reader once the recording is regenerated for a new KiCad.

#[test]
fn reads_the_divider_as_drawn() {
    let design =
        export_design(Path::new(&format!("{DIVIDER}/divider.kicad_sch")))
            .unwrap();
    use PinKind::Passive as P;

    assert_eq!(design.rails, ["+5V", "GND"]);
    assert_eq!(
        parts_list(&design),
        [
            ("R1", "10k", "Device:R_US".into()),
            ("R2", "10k", "Device:R_US".into()),
        ]
    );
    assert_eq!(
        connectivity(&design),
        BTreeMap::from([
            ("+5V", vec![("R1", "1", P)]),
            ("/out", vec![("R1", "2", P), ("R2", "1", P)]),
            ("GND", vec![("R2", "2", P)]),
        ])
    );
}

#[test]
fn reads_the_colpitts_as_drawn() {
    let design =
        export_design(Path::new(&format!("{COLPITTS}/colpitts.kicad_sch")))
            .unwrap();
    use PinKind::{Input as I, Passive as P};

    assert_eq!(design.rails, ["GND", "VCC"]);
    assert_eq!(
        parts_list(&design),
        [
            ("C1", "470pF", "Device:C".into()),
            ("C2", "470pF", "Device:C".into()),
            ("C3", "100nF", "Device:C".into()),
            ("L1", "1uH", "Device:L".into()),
            ("PRB1", "COLPITTS_OUT", "kitest:Probe".into()),
            ("Q1", "2N3904", "Transistor_BJT:2N3904".into()),
            ("R1", "47k", "Device:R_US".into()),
            ("R2", "47k", "Device:R_US".into()),
            ("R3", "4.7k", "Device:R_US".into()),
        ]
    );
    assert_eq!(
        connectivity(&design),
        BTreeMap::from([
            (
                "/OUT",
                vec![
                    ("C1", "2", P),
                    ("C2", "1", P),
                    ("PRB1", "1", P),
                    ("Q1", "1", P),
                    ("R3", "1", P),
                ]
            ),
            (
                "GND",
                vec![
                    ("C2", "2", P),
                    ("C3", "2", P),
                    ("R2", "2", P),
                    ("R3", "2", P)
                ]
            ),
            ("Net-(C3-Pad1)", vec![("C3", "1", P), ("L1", "2", P)]),
            (
                "Net-(Q1-B)",
                vec![
                    ("C1", "1", P),
                    ("L1", "1", P),
                    ("Q1", "2", I),
                    ("R1", "2", P),
                    ("R2", "1", P),
                ]
            ),
            ("VCC", vec![("Q1", "3", P), ("R1", "1", P)]),
        ])
    );

    let q1 = design.component("Q1").unwrap();
    let part = design.part(q1).unwrap();
    let pins: Vec<_> = part
        .pins
        .iter()
        .map(|pin| (pin.number.as_str(), pin.name.as_str(), pin.kind))
        .collect();
    assert_eq!(pins, [("1", "E", P), ("2", "B", I), ("3", "C", P)]);
    for fields in [&part.fields, &q1.fields] {
        assert_eq!(fields["Sim.Device"], "NPN");
        assert_eq!(fields["Sim.Pins"], "1=E 2=B 3=C");
    }

    let probe = design.component("PRB1").unwrap();
    assert!(probe.fields.contains_key("Expect"));
    assert!(probe.excluded_from_sim);
    assert!(!design.net("VCC").unwrap().is_driven());
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
fn an_oscillation_check_on_a_crystal_design_says_it_is_unsupported() {
    let mut design = colpitts_expecting("oscillates(near=10.115e6, within=2%)");
    let mut crystal = design.components[0].clone();
    crystal.reference = "Y1".into();
    crystal.excluded_from_sim = true;
    design.components.push(crystal);
    let config = Config::for_project(Path::new(COLPITTS)).unwrap();
    let report = design.check(&config, &Ngspice::default()).unwrap();

    let check = report.outcomes[0].check.as_ref().expect("a check ran");
    assert!(!check.passed());
    assert!(check.message().starts_with("Y1 is a crystal"), "{check}");
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
