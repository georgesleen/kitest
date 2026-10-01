use kitest::{Backend, ModelLibrary, Ngspice, Tran, TranSource};

const COLPITTS: &str = include_str!("../../../examples/spice/colpitts.cir");

/// The fixture with its own transistor card swapped for the bundled one.
fn colpitts_on_the_bundled_card() -> String {
    let library = ModelLibrary::bundled();
    let entry = library.find("2N3904").expect("2N3904 bundled");
    let body: String = COLPITTS
        .lines()
        .filter(|line| !line.starts_with(".model") && !line.starts_with('+'))
        .map(|line| line.replace(" q2n3904", " 2N3904"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{body}\n{}", entry.card)
}

#[test]
fn the_bundled_2n3904_card_runs_the_colpitts() {
    let deck = colpitts_on_the_bundled_card();
    assert!(!deck.contains("q2n3904"), "fixture card fully replaced");

    let tone = Ngspice::default()
        .run_tran(
            &deck,
            &[TranSource::dc("vcc", 9.0)],
            Tran::new(1e-9, 25e-6).start(5e-6),
        )
        .expect("simulation ran")
        .node("base")
        .expect("base present")
        .dominant_tone();

    let hertz = tone.frequency().hertz();
    assert!((hertz - 10.116e6).abs() / 10.116e6 < 0.01, "{hertz}");
    assert!(tone.amplitude().volts() > 1.0);
}
