//! Measurements of sampled curves, shared by kitest's checks and its scope.

use std::iter;
use std::ops::RangeInclusive;

/// The drop, in decibels, from a response's peak to its half-power points.
pub const HALF_POWER_DB: f64 = 3.010_299_956_639_812;

/// A sampled curve: y over x, with x ascending.
#[derive(Debug, Clone, Copy)]
pub struct Curve<'a> {
    x: &'a [f64],
    y: &'a [f64],
}

/// One crossing of a level: where, and in which direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    pub x: f64,
    pub rising: bool,
}

impl<'a> Curve<'a> {
    /// A curve through the points `(x[i], y[i])`.
    ///
    /// # Panics
    ///
    /// Panics if `x` and `y` differ in length.
    pub fn new(x: &'a [f64], y: &'a [f64]) -> Self {
        assert_eq!(x.len(), y.len(), "x and y differ in length");
        Self { x, y }
    }

    /// The x axis.
    pub fn x(&self) -> &'a [f64] {
        self.x
    }

    /// The y values.
    pub fn y(&self) -> &'a [f64] {
        self.y
    }

    /// The linearly interpolated y at `x`, or `None` outside the curve.
    pub fn at(&self, x: f64) -> Option<f64> {
        let k = self.x.partition_point(|&v| v < x);
        let (&x1, &y1) = (self.x.get(k)?, self.y.get(k)?);
        if x1 == x {
            return Some(y1);
        }
        if k == 0 {
            return None;
        }
        let (x0, y0) = (self.x[k - 1], self.y[k - 1]);
        Some(y0 + (y1 - y0) * (x - x0) / (x1 - x0))
    }

    /// The smallest y over `over`.
    pub fn min(&self, over: RangeInclusive<f64>) -> Option<f64> {
        self.window(over)?.map(|(_, y)| y).reduce(f64::min)
    }

    /// The largest y over `over`.
    pub fn max(&self, over: RangeInclusive<f64>) -> Option<f64> {
        self.window(over)?.map(|(_, y)| y).reduce(f64::max)
    }

    /// The span from the smallest to the largest y over `over`.
    pub fn peak_to_peak(&self, over: RangeInclusive<f64>) -> Option<f64> {
        Some(self.max(over.clone())? - self.min(over)?)
    }

    /// The mean of y over `over`, weighted by x.
    pub fn mean(&self, over: RangeInclusive<f64>) -> Option<f64> {
        self.average(over, |a, b| (a + b) / 2.0)
    }

    /// The root mean square of y over `over`, weighted by x.
    pub fn rms(&self, over: RangeInclusive<f64>) -> Option<f64> {
        self.average(over, |a, b| (a * a + a * b + b * b) / 3.0)
            .map(f64::sqrt)
    }

    /// Every place where y crosses `level` over `over`.
    ///
    /// A run of samples exactly on `level` counts as one crossing, at its
    /// first sample, when the curve leaves on the other side.
    pub fn crossings(
        &self,
        level: f64,
        over: RangeInclusive<f64>,
    ) -> Vec<Crossing> {
        let mut found = Vec::new();
        let Some(points) = self.window(over) else {
            return found;
        };
        let mut side = 0;
        let mut touch = None;
        let mut prev = (f64::NAN, f64::NAN);
        for (x, y) in points {
            let s = if y > level {
                1
            } else if y < level {
                -1
            } else {
                0
            };
            if s == 0 {
                touch.get_or_insert(x);
            } else {
                if side != 0 && s != side {
                    let (x0, y0) = prev;
                    let at = touch.unwrap_or_else(|| {
                        x0 + (level - y0) * (x - x0) / (y - y0)
                    });
                    found.push(Crossing {
                        x: at,
                        rising: s > 0,
                    });
                }
                side = s;
                touch = None;
            }
            prev = (x, y);
        }
        found
    }

    /// The repetition rate over `over`, in cycles per unit of x.
    ///
    /// Counts rising crossings of the midpoint between the smallest and the
    /// largest y, and returns `None` under two of them.
    pub fn frequency(&self, over: RangeInclusive<f64>) -> Option<f64> {
        let (min, max) = (self.min(over.clone())?, self.max(over.clone())?);
        if max <= min {
            return None;
        }
        let rising: Vec<f64> = self
            .crossings((min + max) / 2.0, over)
            .into_iter()
            .filter(|c| c.rising)
            .map(|c| c.x)
            .collect();
        match rising[..] {
            [first, .., last] => {
                Some((rising.len() - 1) as f64 / (last - first))
            }
            _ => None,
        }
    }

    /// The x taken by the first rising edge over `over` from 10% to 90% of
    /// the span between the smallest and the largest y.
    pub fn rise_time(&self, over: RangeInclusive<f64>) -> Option<f64> {
        let (low, high) = self.edge_levels(over.clone())?;
        let top = self
            .crossings(high, over.clone())
            .into_iter()
            .find(|c| c.rising)?
            .x;
        let bottom = self
            .crossings(low, over)
            .into_iter()
            .rev()
            .find(|c| c.rising && c.x <= top)?
            .x;
        Some(top - bottom)
    }

    /// The x taken by the first falling edge over `over` from 90% to 10% of
    /// the span between the smallest and the largest y.
    pub fn fall_time(&self, over: RangeInclusive<f64>) -> Option<f64> {
        let (low, high) = self.edge_levels(over.clone())?;
        let bottom = self
            .crossings(low, over.clone())
            .into_iter()
            .find(|c| !c.rising)?
            .x;
        let top = self
            .crossings(high, over)
            .into_iter()
            .rev()
            .find(|c| !c.rising && c.x <= bottom)?
            .x;
        Some(bottom - top)
    }

    /// The largest y over `over` above `target`, as a fraction of `target`.
    ///
    /// Returns 0 when y stays at or below `target`.
    pub fn overshoot(
        &self,
        target: f64,
        over: RangeInclusive<f64>,
    ) -> Option<f64> {
        let peak = self.max(over)?;
        Some(((peak - target) / target.abs()).max(0.0))
    }

    /// The largest distance of y from `target` over `over`.
    pub fn worst_deviation(
        &self,
        target: f64,
        over: RangeInclusive<f64>,
    ) -> Option<f64> {
        self.window(over)?
            .map(|(_, y)| (y - target).abs())
            .reduce(f64::max)
    }

    /// Every x over `over` where y crosses [`HALF_POWER_DB`] below its
    /// largest value.
    ///
    /// y is in decibels.
    pub fn half_power_points(&self, over: RangeInclusive<f64>) -> Vec<f64> {
        let Some(peak) = self.max(over.clone()) else {
            return Vec::new();
        };
        self.crossings(peak - HALF_POWER_DB, over)
            .into_iter()
            .map(|c| c.x)
            .collect()
    }

    /// The points of the curve clipped to `over`, with the ends interpolated.
    fn window(
        &self,
        over: RangeInclusive<f64>,
    ) -> Option<impl Iterator<Item = (f64, f64)> + 'a> {
        let (&first, &last) = (self.x.first()?, self.x.last()?);
        let (start, end) = over.into_inner();
        let (lo, hi) = (start.max(first), end.min(last));
        if start.is_nan() || end.is_nan() || lo > hi {
            return None;
        }
        let head = (lo, self.at(lo)?);
        let tail = (hi > lo).then_some((hi, self.at(hi)?));
        let i = self.x.partition_point(|&x| x <= lo);
        let j = self.x.partition_point(|&x| x < hi).max(i);
        let inner = self.x[i..j]
            .iter()
            .copied()
            .zip(self.y[i..j].iter().copied());
        Some(iter::once(head).chain(inner).chain(tail))
    }

    /// The x-weighted average over `over` of `segment`, the mean of a
    /// function of y across one linear segment from `a` to `b`.
    fn average(
        &self,
        over: RangeInclusive<f64>,
        segment: impl Fn(f64, f64) -> f64,
    ) -> Option<f64> {
        let mut points = self.window(over)?;
        let (start, mut y0) = points.next()?;
        let (mut x0, mut sum) = (start, 0.0);
        for (x1, y1) in points {
            sum += segment(y0, y1) * (x1 - x0);
            (x0, y0) = (x1, y1);
        }
        let width = x0 - start;
        (width > 0.0).then(|| sum / width)
    }

    /// The 10% and 90% levels of the span of y over `over`.
    fn edge_levels(&self, over: RangeInclusive<f64>) -> Option<(f64, f64)> {
        let (min, max) = (self.min(over.clone())?, self.max(over)?);
        let span = max - min;
        (span > 0.0).then_some((min + 0.1 * span, min + 0.9 * span))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{LN_10, PI};

    const ALL: RangeInclusive<f64> = f64::NEG_INFINITY..=f64::INFINITY;

    /// `n + 1` x values from 0 to `end`, spaced unevenly.
    fn uneven(end: f64, n: usize) -> Vec<f64> {
        (0..=n)
            .map(|i| {
                let u = i as f64 / n as f64;
                end * (u + 0.3 * (2.0 * PI * u * 7.0).sin() / (2.0 * PI * 7.0))
            })
            .collect()
    }

    fn close(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} is not within {tolerance} of {expected}"
        );
    }

    #[test]
    fn at_interpolates_and_rejects_outside() {
        let (x, y) = ([0.0, 1.0, 3.0], [0.0, 2.0, 0.0]);
        let c = Curve::new(&x, &y);
        assert_eq!(c.at(0.5), Some(1.0));
        assert_eq!(c.at(2.0), Some(1.0));
        assert_eq!(c.at(3.0), Some(0.0));
        assert_eq!(c.at(-0.1), None);
        assert_eq!(c.at(3.1), None);
        assert_eq!(c.at(f64::NAN), None);
    }

    #[test]
    fn window_edges_are_interpolated() {
        let (x, y) = ([0.0, 1.0, 2.0], [0.0, 1.0, 2.0]);
        let c = Curve::new(&x, &y);
        assert_eq!(c.min(0.25..=1.75), Some(0.25));
        assert_eq!(c.max(0.25..=1.75), Some(1.75));
        assert_eq!(c.peak_to_peak(0.25..=1.75), Some(1.5));
        close(c.mean(0.25..=1.75).unwrap(), 1.0, 1e-12);
        assert_eq!(c.worst_deviation(0.0, 0.5..=0.5), Some(0.5));
    }

    #[test]
    fn empty_and_outside_windows_measure_nothing() {
        let (x, y) = ([0.0, 1.0, 2.0], [0.0, 1.0, 0.0]);
        let c = Curve::new(&x, &y);
        let empty = Curve::new(&[], &[]);
        assert_eq!(empty.max(ALL), None);
        assert_eq!(c.max(3.0..=4.0), None);
        assert_eq!(c.min(-2.0..=-1.0), None);
        assert_eq!(c.max(1.5..=0.5), None);
        assert_eq!(c.max(f64::NAN..=1.0), None);
        assert_eq!(c.mean(1.0..=1.0), None);
        assert_eq!(c.rms(3.0..=4.0), None);
        assert_eq!(c.overshoot(1.0, 3.0..=4.0), None);
        assert!(c.crossings(0.5, 3.0..=4.0).is_empty());
        assert!(c.half_power_points(3.0..=4.0).is_empty());
    }

    #[test]
    fn mean_and_rms_of_a_ramp_on_uneven_steps() {
        let x = uneven(2.0, 13);
        let c = Curve::new(&x, &x);
        close(c.mean(ALL).unwrap(), 1.0, 1e-12);
        close(c.rms(ALL).unwrap(), (4.0_f64 / 3.0).sqrt(), 1e-12);
    }

    #[test]
    fn mean_and_rms_of_a_sine_on_uneven_steps() {
        let x = uneven(3.0, 3000);
        let y: Vec<f64> =
            x.iter().map(|t| 2.0 + (2.0 * PI * t).sin()).collect();
        let c = Curve::new(&x, &y);
        close(c.mean(ALL).unwrap(), 2.0, 1e-5);
        close(c.rms(ALL).unwrap(), 4.5_f64.sqrt(), 1e-5);
    }

    #[test]
    fn crossings_are_interpolated_with_direction() {
        let (x, y) = ([0.0, 1.0, 2.0, 3.0], [0.0, 2.0, 2.0, -2.0]);
        let c = Curve::new(&x, &y);
        assert_eq!(
            c.crossings(1.0, ALL),
            [
                Crossing {
                    x: 0.5,
                    rising: true
                },
                Crossing {
                    x: 2.25,
                    rising: false
                },
            ]
        );
    }

    #[test]
    fn a_run_on_the_level_counts_once_and_a_touch_not_at_all() {
        let x = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let y = [-1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0];
        let c = Curve::new(&x, &y);
        assert_eq!(
            c.crossings(0.0, ALL),
            [Crossing {
                x: 1.0,
                rising: true
            }]
        );
    }

    #[test]
    fn frequency_of_a_sampled_sine() {
        let x = uneven(5e-3, 4000);
        let y: Vec<f64> =
            x.iter().map(|t| (2.0 * PI * 1e3 * t + 0.4).sin()).collect();
        let c = Curve::new(&x, &y);
        close(c.frequency(ALL).unwrap(), 1e3, 1e-2);
    }

    #[test]
    fn frequency_of_a_square() {
        let x: Vec<f64> = (0..=4000).map(|i| i as f64 * 1e-6).collect();
        let y: Vec<f64> = x
            .iter()
            .map(|t| {
                if (t * 1e3 + 0.1005).fract() < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            })
            .collect();
        let c = Curve::new(&x, &y);
        close(c.frequency(ALL).unwrap(), 1e3, 1e-6);
        assert_eq!(c.frequency(0.0..=0.8e-3), None);
    }

    #[test]
    fn rise_and_fall_of_a_sampled_rc_step() {
        let tau = 1e-3;
        let x = uneven(20.0 * tau, 4000);
        let y: Vec<f64> = x
            .iter()
            .map(|&t| {
                if t < 10.0 * tau {
                    1.0 - (-t / tau).exp()
                } else {
                    (-(t - 10.0 * tau) / tau).exp()
                }
            })
            .collect();
        let c = Curve::new(&x, &y);
        close(c.rise_time(ALL).unwrap(), 2.197 * tau, 1e-3 * tau);
        close(c.fall_time(ALL).unwrap(), 2.197 * tau, 1e-3 * tau);
    }

    #[test]
    fn rise_time_needs_a_whole_edge() {
        let (x, y) = ([0.0, 1.0, 2.0], [1.0, 1.0, 1.0]);
        assert_eq!(Curve::new(&x, &y).rise_time(ALL), None);
        let (x, y) = ([0.0, 1.0], [1.0, 0.0]);
        assert_eq!(Curve::new(&x, &y).rise_time(ALL), None);
    }

    #[test]
    fn overshoot_and_worst_deviation() {
        let (x, y) = ([0.0, 1.0, 2.0, 3.0], [0.0, 1.25, 0.9, 1.0]);
        let c = Curve::new(&x, &y);
        close(c.overshoot(1.0, ALL).unwrap(), 0.25, 1e-12);
        assert_eq!(c.overshoot(2.0, ALL), Some(0.0));
        close(c.worst_deviation(1.0, 2.0..=3.0).unwrap(), 0.1, 1e-12);
        close(c.worst_deviation(1.0, 1.5..=3.0).unwrap(), 0.1, 1e-12);
    }

    #[test]
    fn half_power_point_of_a_first_order_low_pass() {
        let cutoff: f64 = 1e3;
        let x: Vec<f64> = (0..=600).map(|i| i as f64 / 100.0).collect();
        let y: Vec<f64> = x
            .iter()
            .map(|lf| {
                let r = (lf * LN_10).exp() / cutoff;
                -10.0 * (1.0 + r * r).log10()
            })
            .collect();
        let c = Curve::new(&x, &y);
        let points = c.half_power_points(ALL);
        assert_eq!(points.len(), 1);
        close(points[0], cutoff.log10(), 1e-3);
    }
}
