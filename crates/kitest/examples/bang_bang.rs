//! A bang-bang controller holds an RC node near a setpoint through a live session.

use kitest::{Drive, Ngspice, SessionError};

/// `vin` drives `vout` through 1 kOhm into 1 uF.
const RC: &str = include_str!("../../../examples/spice/rc.cir");

const SUPPLY: f64 = 1.0;
const SETPOINT: f64 = 0.6;
const HYSTERESIS: f64 = 0.02;
const PERIOD: f64 = 50e-6;
const STOP: f64 = 5e-3;
const PRINT_EVERY: usize = 4;

fn main() -> Result<(), SessionError> {
    let mut session = Ngspice::default().session(
        RC,
        &[Drive::new("vin", 0.0)],
        STOP,
        10e-6,
    )?;
    let mut drive = 0.0;
    let mut switches = 0;
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut settled = false;

    println!("{:>10} {:>10} {:>6}", "time_s", "vout_V", "vin_V");
    let ticks = (STOP / PERIOD).round() as usize;
    for tick in 1..=ticks {
        session.advance_to(tick as f64 * PERIOD)?;
        let vout = session.node("vout")?.volts();
        if settled {
            low = low.min(vout);
            high = high.max(vout);
        }
        let next = if vout < SETPOINT - HYSTERESIS {
            SUPPLY
        } else if vout > SETPOINT + HYSTERESIS {
            settled = true;
            0.0
        } else {
            drive
        };
        if next != drive && tick < ticks {
            session.set("vin", next)?;
            drive = next;
            switches += 1;
        }
        if tick % PRINT_EVERY == 0 {
            println!("{:>10.6} {:>10.6} {:>6.1}", session.time(), vout, drive);
        }
    }
    session.finish()?;

    println!(
        "{switches} switches; after the first overshoot vout stays in [{low:.4}, {high:.4}] V for a band of [{:.4}, {:.4}] V",
        SETPOINT - HYSTERESIS,
        SETPOINT + HYSTERESIS,
    );
    Ok(())
}
