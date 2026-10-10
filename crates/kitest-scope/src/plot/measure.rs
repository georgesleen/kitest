//! What a pane can measure of each of its channels over a window.

use std::ops::RangeInclusive;

use kitest_measure::Curve;
use serde::{Deserialize, Serialize};

/// One measurement of a channel over a window.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Measurement {
    Min,
    Max,
    PeakToPeak,
    Mean,
    Rms,
    Frequency,
    RiseTime,
    FallTime,
    HalfPower,
}

/// A measurement's result, by what it measures.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A level, in the channel's y unit.
    Level(f64),
    /// A length of x, in the x unit.
    Duration(f64),
    /// A repetition rate, in hertz.
    Rate(f64),
    /// Places on the x axis, in plot coordinates.
    Positions(Vec<f64>),
}

/// What a voltage over time can be measured for.
pub const WAVEFORM: &[Measurement] = &[
    Measurement::Min,
    Measurement::Max,
    Measurement::PeakToPeak,
    Measurement::Mean,
    Measurement::Rms,
    Measurement::Frequency,
    Measurement::RiseTime,
    Measurement::FallTime,
];

/// What a magnitude in decibels over frequency can be measured for.
pub const MAGNITUDE: &[Measurement] =
    &[Measurement::Max, Measurement::Min, Measurement::HalfPower];

/// What a phase over frequency can be measured for.
pub const PHASE: &[Measurement] = &[Measurement::Max, Measurement::Min];

/// What group delay over frequency can be measured for.
pub const GROUP_DELAY: &[Measurement] =
    &[Measurement::Min, Measurement::Max, Measurement::PeakToPeak];

impl Measurement {
    /// The measurement's short name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Min => "min",
            Self::Max => "max",
            Self::PeakToPeak => "peak to peak",
            Self::Mean => "mean",
            Self::Rms => "RMS",
            Self::Frequency => "frequency",
            Self::RiseTime => "rise time",
            Self::FallTime => "fall time",
            Self::HalfPower => "-3 dB points",
        }
    }

    /// The measurement of `curve` over `over`, or `None` when the window does
    /// not define it.
    pub fn of(
        self,
        curve: Curve<'_>,
        over: RangeInclusive<f64>,
    ) -> Option<Value> {
        Some(match self {
            Self::Min => Value::Level(curve.min(over)?),
            Self::Max => Value::Level(curve.max(over)?),
            Self::PeakToPeak => Value::Level(curve.peak_to_peak(over)?),
            Self::Mean => Value::Level(curve.mean(over)?),
            Self::Rms => Value::Level(curve.rms(over)?),
            Self::Frequency => Value::Rate(curve.frequency(over)?),
            Self::RiseTime => Value::Duration(curve.rise_time(over)?),
            Self::FallTime => Value::Duration(curve.fall_time(over)?),
            Self::HalfPower => {
                let points = curve.half_power_points(over);
                if points.is_empty() {
                    return None;
                }
                Value::Positions(points)
            }
        })
    }
}
