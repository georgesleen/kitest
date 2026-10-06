//! A part bound to the model its KiCad `Sim.Library` field names.

use std::path::Path;

use kitest::{Config, ModelLibrary, Ngspice, export_design};

const COLPITTS: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/kicad/colpitts");

/// The bundled 2N3904, renamed so only the project's file can supply it.
const VENDOR_MODEL: &str = "* vendor model\n\
.model Q2N3904_VENDOR npn(is=6.734f xti=3 eg=1.11 vaf=74.03 bf=416.4\n\
+ ne=1.259 ise=6.734f ikf=66.78m xtb=1.5 br=.7371 nc=2 isc=0 ikr=0 rc=1\n\
+ cjc=3.638p mjc=.3085 vjc=.75 fc=.5 cje=4.493p mje=.2593 vje=.75\n\
+ tr=239.5n tf=301.2p itf=.4 vtf=4 xtf=2 rb=10)\n";

/// The colpitts copied to a temporary project.
fn colpitts_copy() -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(COLPITTS).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            let to = project.path().join(entry.file_name());
            std::fs::copy(entry.path(), to).unwrap();
        }
    }
    project
}

/// The colpitts copied to a temporary project whose Q1 names `models/q.lib`.
fn project_with_model_file() -> tempfile::TempDir {
    let project = colpitts_copy();
    let models = project.path().join("models");
    std::fs::create_dir(&models).unwrap();
    std::fs::write(models.join("q.lib"), VENDOR_MODEL).unwrap();
    let sch = project.path().join("colpitts.kicad_sch");
    add_model_fields(&sch, "${KIPRJMOD}/models/q.lib", "Q2N3904_VENDOR");
    project
}

/// Give Q1 in `sch` a Sim.Library and Sim.Name, beside its Sim.Device.
fn add_model_fields(sch: &Path, library: &str, name: &str) {
    let text = std::fs::read_to_string(sch).unwrap();
    let q1 = text.find("(property \"Reference\" \"Q1\"").unwrap();
    let at = q1 + text[q1..].find("(property \"Sim.Device\"").unwrap();
    let fields = format!(
        "(property \"Sim.Library\" \"{library}\" (at 0 0 0) (hide yes))\n\t\t\
         (property \"Sim.Name\" \"{name}\" (at 0 0 0) (hide yes))\n\t\t"
    );
    let edited = format!("{}{fields}{}", &text[..at], &text[at..]);
    std::fs::write(sch, edited).unwrap();
}

#[test]
fn a_path_through_a_variable_only_kicad_sets_resolves_as_kicad_resolves_it() {
    let project = colpitts_copy();
    let sch = project.path().join("colpitts.kicad_sch");
    let library = "${KICAD9_SYMBOL_DIR}/Simulation_SPICE.sp";
    add_model_fields(&sch, library, "kicad_builtin_vdiff");
    assert!(std::env::var_os("KICAD9_SYMBOL_DIR").is_none());

    let design = export_design(&sch).unwrap();
    let q1 = design
        .components
        .iter()
        .find(|component| component.reference == "Q1")
        .unwrap();
    let path = Path::new(&q1.fields["Sim.Library"]);
    assert!(path.ends_with("Simulation_SPICE.sp"), "{}", path.display());
    assert!(path.is_file(), "{}", path.display());
}

#[test]
fn a_kicad_sim_library_model_drives_the_netlist_and_the_check() {
    let project = project_with_model_file();
    let sch = project.path().join("colpitts.kicad_sch");
    let mut design = export_design(&sch).unwrap();
    let probe = design
        .components
        .iter_mut()
        .find(|component| component.reference == "PRB1")
        .unwrap();
    probe.fields.insert(
        "Expect".into(),
        "oscillates(near=10.115MHz, within=2%)".into(),
    );
    let netlist = design.netlist(&[ModelLibrary::bundled()]).unwrap();

    let q1 = netlist.text.lines().find(|line| line.starts_with("Q1"));
    assert!(q1.unwrap().ends_with("Q2N3904_VENDOR"), "{}", netlist.text);
    let include = format!("{}", project.path().join("models/q.lib").display());
    assert!(netlist.text.contains(&include), "{}", netlist.text);

    let config = Config::for_project(project.path()).unwrap();
    let report = design.check(&config, &Ngspice::default()).unwrap();
    let check = report.outcomes[0].check.as_ref().expect("a check ran");
    assert!(check.passed(), "{check}");
}
