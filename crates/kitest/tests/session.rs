use kitest::{Drive, Ngspice, Session, SessionError};

/// `vin` drives `vout` through 1 kOhm into 1 uF.
const RC: &str = include_str!("../../../examples/spice/rc.cir");
const TAU: f64 = 1e-3;
const STOP: f64 = 10e-3;
const MAX_STEP: f64 = 10e-6;

/// Agreement between a read and the closed-form RC charge.
const VOLTS: f64 = 1e-5;

/// Relative agreement between a requested and a landed time.
const LANDING: f64 = 1e-12;

fn rc_session() -> Session {
    Ngspice::default()
        .session(RC, &[Drive::new("vin", 0.0)], STOP, MAX_STEP)
        .unwrap()
}

fn read(session: &mut Session, node: &str) -> f64 {
    session.node(node).unwrap().volts()
}

/// Voltage on the capacitor `elapsed` seconds after `vin` steps to `volts`
/// from an uncharged state.
fn charge(volts: f64, elapsed: f64) -> f64 {
    volts * (1.0 - (-elapsed / TAU).exp())
}

fn assert_landed(session: &Session, requested: f64) {
    let landed = session.time();
    assert!(
        (landed - requested).abs() <= LANDING * requested,
        "landed at {landed} s, requested {requested} s"
    );
}

#[test]
fn step_charges_the_capacitor_as_closed_form() {
    let mut session = rc_session();
    let step = 1.2345e-3;
    session.advance_to(step).unwrap();
    assert_landed(&session, step);
    assert_eq!(read(&mut session, "vout"), 0.0);

    session.set("vin", 1.0).unwrap();
    // Off the 10 us internal step grid, so each read needs its own landing.
    for time in [1.5e-3, 2.0001e-3, 3.7777e-3, 6.54321e-3, STOP] {
        session.advance_to(time).unwrap();
        assert_landed(&session, time);
        let expected = charge(1.0, time - step);
        let vout = read(&mut session, "vout");
        assert!(
            (vout - expected).abs() <= VOLTS,
            "v(vout) at {time} s is {vout}, expected {expected}"
        );
    }
    session.finish().unwrap();
}

#[test]
fn step_applies_at_its_time() {
    let mut session = rc_session();
    let step = 1e-3;
    session.advance_to(step).unwrap();
    session.set("vin", 1.0).unwrap();
    // A read at the time of a change sees the value before it.
    assert_eq!(read(&mut session, "vin"), 0.0);

    // One microsecond is a tenth of the internal step. An edge applied a
    // step late leaves vout at zero, and one interpolated from the step
    // before raises it by about 5 mV.
    let after = step + 1e-6;
    session.advance_to(after).unwrap();
    assert_eq!(read(&mut session, "vin"), 1.0);
    let expected = charge(1.0, after - step);
    let vout = read(&mut session, "vout");
    assert!(
        (vout - expected).abs() <= 1e-8,
        "v(vout) just after the step is {vout}, expected {expected}"
    );
    session.finish().unwrap();
}

#[test]
fn reads_time_zero_from_the_operating_point() {
    let mut session = Ngspice::default()
        .session(RC, &[Drive::new("vin", 2.0)], STOP, MAX_STEP)
        .unwrap();
    assert_eq!(session.time(), 0.0);
    assert_eq!(read(&mut session, "vout"), 2.0);
    session.finish().unwrap();
}

#[test]
fn sessions_run_concurrently_without_interference() {
    let run = |volts: f64, step: f64| {
        std::thread::spawn(move || {
            let mut session = rc_session();
            session.advance_to(step).unwrap();
            session.set("vin", volts).unwrap();
            let mut reads = Vec::new();
            for index in 1..=20 {
                let time = step + index as f64 * 0.2e-3;
                session.advance_to(time).unwrap();
                reads.push((time, read(&mut session, "vout")));
            }
            session.finish().unwrap();
            (volts, step, reads)
        })
    };
    let handles = [run(1.0, 0.5e-3), run(-3.0, 1.1e-3), run(5.0, 2.3e-3)];
    for handle in handles {
        let (volts, step, reads) = handle.join().unwrap();
        for (time, vout) in reads {
            let expected = charge(volts, time - step);
            assert!(
                (vout - expected).abs() <= VOLTS * volts.abs(),
                "{volts} V session: v(vout) at {time} s is {vout}, expected {expected}"
            );
        }
    }
}

#[test]
fn going_back_in_time_is_an_error() {
    let mut session = rc_session();
    session.advance_to(2e-3).unwrap();
    assert!(matches!(
        session.advance_to(1e-3),
        Err(SessionError::Backwards { .. })
    ));
}

#[test]
fn advancing_to_the_current_time_does_nothing() {
    let mut session = rc_session();
    session.advance_to(2e-3).unwrap();
    session.advance_to(2e-3).unwrap();
    assert_landed(&session, 2e-3);
}

#[test]
fn an_advance_below_the_resolution_is_an_error() {
    let mut session = rc_session();
    session.advance_to(1e-3).unwrap();
    assert!(matches!(
        session.advance_to(1e-3 + 1e-9),
        Err(SessionError::TooClose { .. })
    ));
    // The session is still usable at its old time.
    session.advance_to(2e-3).unwrap();
    assert_landed(&session, 2e-3);
}

#[test]
fn advancing_past_the_stop_time_is_an_error() {
    let mut session = rc_session();
    assert!(matches!(
        session.advance_to(2.0 * STOP),
        Err(SessionError::PastStop { .. })
    ));
}

#[test]
fn a_non_finite_time_is_an_error() {
    let mut session = rc_session();
    assert!(matches!(
        session.advance_to(f64::NAN),
        Err(SessionError::NotFinite { .. })
    ));
}

#[test]
fn setting_an_undeclared_drive_is_an_error() {
    let mut session = rc_session();
    session.advance_to(1e-3).unwrap();
    assert!(matches!(
        session.set("vout", 1.0),
        Err(SessionError::UnknownDrive { .. })
    ));
}

#[test]
fn setting_a_drive_at_time_zero_is_an_error() {
    let mut session = rc_session();
    assert!(matches!(
        session.set("vin", 1.0),
        Err(SessionError::AtStart)
    ));
}

#[test]
fn reading_an_absent_node_is_an_error() {
    let mut session = rc_session();
    assert!(matches!(
        session.node("nowhere"),
        Err(SessionError::UnknownNode { .. })
    ));
    session.advance_to(1e-3).unwrap();
    assert!(matches!(
        session.node("nowhere"),
        Err(SessionError::UnknownNode { .. })
    ));
}

#[test]
fn a_broken_netlist_is_an_error() {
    let result = Ngspice::default().session(
        "* broken\nr1 vin\n",
        &[Drive::new("vin", 0.0)],
        STOP,
        MAX_STEP,
    );
    assert!(
        matches!(result, Err(SessionError::Command { .. })),
        "{:?}",
        result.err()
    );
}
