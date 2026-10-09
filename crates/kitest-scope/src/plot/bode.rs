//! The Bode view: each AC trace's magnitude and unwrapped phase against
//! frequency, on a log axis.

use kitest_scope::AcTrace;

use super::measure::{
    MAGNITUDE as MAGNITUDE_MEASUREMENTS, PHASE as PHASE_MEASUREMENTS,
};
use super::{Channel, Quantity, Unit, View};

/// The x axis: frequency, in hertz, on a log scale.
const FREQUENCY: Quantity = Quantity {
    name: "frequency",
    unit: Unit::Si("Hz"),
    log: true,
    measurements: &[],
};

/// A trace's magnitude channel, in decibels.
const MAGNITUDE: Quantity = Quantity {
    name: "magnitude",
    unit: Unit::Plain(" dB"),
    log: false,
    measurements: MAGNITUDE_MEASUREMENTS,
};

/// A trace's phase channel, in degrees.
const PHASE: Quantity = Quantity {
    name: "phase",
    unit: Unit::Plain("°"),
    log: false,
    measurements: PHASE_MEASUREMENTS,
};

/// A magnitude pane above a phase pane, with one channel of each per trace
/// against `frequency` in hertz.
pub fn view(frequency: &[f64], traces: &[AcTrace]) -> View {
    let mut magnitudes = Vec::new();
    let mut phases = Vec::new();
    for (index, trace) in traces.iter().enumerate() {
        let (x, magnitude, phase) = response(frequency, trace);
        magnitudes.push(Channel::new(
            &trace.name,
            index,
            MAGNITUDE,
            x.clone(),
            magnitude,
        ));
        phases.push(Channel::new(&trace.name, index, PHASE, x, phase));
    }
    let count = traces.len();
    let channels = magnitudes.into_iter().chain(phases).collect();
    View::new(
        "AC sweep",
        FREQUENCY,
        channels,
        vec![(0..count).collect(), (count..2 * count).collect()],
    )
}

/// The trace's log10 frequencies, magnitudes in decibels, and unwrapped
/// phases in degrees.
///
/// Frequencies at or below zero, which a log axis cannot place, are left out.
fn response(
    frequency: &[f64],
    trace: &AcTrace,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let kept: Vec<usize> = (0..frequency.len())
        .filter(|&index| frequency[index] > 0.0)
        .collect();
    let phase = unwrapped_phase(&trace.re, &trace.im);
    (
        kept.iter().map(|&index| frequency[index].log10()).collect(),
        kept.iter()
            .map(|&index| 20.0 * trace.re[index].hypot(trace.im[index]).log10())
            .collect(),
        kept.iter().map(|&index| phase[index]).collect(),
    )
}

/// The phase of each `re + j im`, in degrees, each within half a turn of the one before.
fn unwrapped_phase(re: &[f64], im: &[f64]) -> Vec<f64> {
    let mut previous: Option<f64> = None;
    re.iter()
        .zip(im)
        .map(|(re, im)| {
            let mut phase = im.atan2(*re).to_degrees();
            if let Some(previous) = previous {
                phase += 360.0 * ((previous - phase) / 360.0).round();
            }
            previous = Some(phase);
            phase
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use kitest_measure::Curve;
    use kitest_scope::AcTrace;

    use super::{response, unwrapped_phase};

    #[test]
    fn an_rc_lowpass_reads_minus_three_db_and_minus_45_degrees_at_its_cutoff() {
        let cutoff = 1.0 / (2.0 * std::f64::consts::PI * 1e3 * 1e-6);
        let frequency: Vec<f64> = (0..=600)
            .map(|i| 10f64.powf(f64::from(i) / 100.0))
            .collect();
        let (re, im) = frequency
            .iter()
            .map(|f| {
                let x = f / cutoff;
                (1.0 / (1.0 + x * x), -x / (1.0 + x * x))
            })
            .unzip();
        let trace = AcTrace {
            name: "vout".into(),
            re,
            im,
        };
        let (x, magnitude, phase) = response(&frequency, &trace);
        let at = cutoff.log10();
        let magnitude = Curve::new(&x, &magnitude).at(at).unwrap();
        let phase = Curve::new(&x, &phase).at(at).unwrap();
        assert!((magnitude + 3.0103).abs() < 0.01, "{magnitude} dB");
        assert!((phase + 45.0).abs() < 0.1, "{phase} degrees");
    }

    #[test]
    fn phase_continues_past_half_a_turn() {
        let degrees = [-170.0_f64, -180.0, -190.0, -350.0, -370.0];
        let (re, im): (Vec<f64>, Vec<f64>) = degrees
            .iter()
            .map(|d| (d.to_radians().cos(), d.to_radians().sin()))
            .unzip();
        let phase = unwrapped_phase(&re, &im);
        for (phase, expected) in phase.iter().zip(degrees) {
            assert!((phase - expected).abs() < 1e-9, "{phase} != {expected}");
        }
    }

    #[test]
    fn frequencies_a_log_axis_cannot_place_are_left_out() {
        let trace = AcTrace {
            name: "vout".into(),
            re: vec![1.0; 3],
            im: vec![0.0; 3],
        };
        let (x, magnitude, phase) = response(&[0.0, 10.0, 100.0], &trace);
        assert_eq!(x, [1.0, 2.0]);
        assert_eq!((magnitude.len(), phase.len()), (2, 2));
    }
}
