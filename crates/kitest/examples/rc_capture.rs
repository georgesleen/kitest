//! Save an RC low-pass's AC sweep and step response as scope files.
//!
//! `cargo run -p kitest --example rc_capture -- DIR` writes `DIR/rc-bode.json`
//! and `DIR/rc-step.json`; open either with `kitest-scope`.

use std::path::PathBuf;

use kitest::{Ac, AcSupply, Backend, Ngspice, Pulse, Sweep, Tran, TranSource};

const RC: &str = include_str!("../../../examples/spice/rc.cir");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(std::env::args_os().nth(1).unwrap_or(".".into()));
    let ngspice = Ngspice::default();

    let ac = ngspice.run_ac(
        RC,
        &[AcSupply::new("vin").magnitude(1.0).bias(0.0)],
        Ac {
            sweep: Sweep::Dec,
            points: 100,
            fstart: 1.0,
            fstop: 1e6,
        },
    )?;
    let bode = dir.join("rc-bode.json");
    ac.capture("rc-bode").save(&bode)?;

    let tran = ngspice.run_tran(
        RC,
        &[TranSource::pulse("vin", Pulse::step(0.0, 1.0))],
        Tran::new(1e-5, 5e-3),
    )?;
    let step = dir.join("rc-step.json");
    tran.capture("rc-step").save(&step)?;

    println!("{}\n{}", bode.display(), step.display());
    Ok(())
}
