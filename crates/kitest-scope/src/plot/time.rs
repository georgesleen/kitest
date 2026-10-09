//! The time view: a transient capture's traces against seconds.

use kitest_scope::Trace;

use super::measure::WAVEFORM;
use super::{Channel, Quantity, Unit, View};

/// The x axis: time, in seconds.
const TIME: Quantity = Quantity {
    name: "time",
    unit: Unit::Si("s"),
    log: false,
    measurements: &[],
};

/// A trace's channel: node voltage, in volts.
const VOLTAGE: Quantity = Quantity {
    name: "voltage",
    unit: Unit::Si("V"),
    log: false,
    measurements: WAVEFORM,
};

/// One voltage channel per trace against `time` in seconds, all in one pane.
pub fn view(time: &[f64], traces: &[Trace]) -> View {
    let channels: Vec<Channel> = traces
        .iter()
        .enumerate()
        .map(|(index, trace)| {
            Channel::new(
                &trace.name,
                index,
                VOLTAGE,
                time.to_vec(),
                trace.values.clone(),
            )
        })
        .collect();
    let all = (0..channels.len()).collect();
    View::new("transient", TIME, channels, vec![all])
}
