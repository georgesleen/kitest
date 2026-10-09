//! Min/max decimation: at most two points per pixel column of a curve.

use std::ops::RangeInclusive;

use egui_plot::{PlotPoint, PlotPoints};

/// The points of a curve that draw it over `x_range` across `columns` pixel columns.
///
/// `points` is sorted by x. The result keeps each column's lowest and highest
/// point, and the nearest point beyond each end of the range.
pub fn visible(
    points: &[PlotPoint],
    x_range: RangeInclusive<f64>,
    columns: usize,
) -> PlotPoints<'_> {
    let (start, end) = (*x_range.start(), *x_range.end());
    let first = points
        .partition_point(|point| point.x < start)
        .saturating_sub(1);
    let last =
        (points.partition_point(|point| point.x <= end) + 1).min(points.len());
    let points = &points[first..last];
    let columns = columns.max(1);
    if points.len() <= 2 * columns || end <= start {
        return PlotPoints::Borrowed(points);
    }

    let width = (end - start) / columns as f64;
    let mut kept = Vec::with_capacity(2 * columns + 4);
    let mut current: Option<(i64, PlotPoint, PlotPoint)> = None;
    for &point in points {
        let column = ((point.x - start) / width).floor() as i64;
        match &mut current {
            Some((index, low, high)) if *index == column => {
                if point.y < low.y {
                    *low = point;
                }
                if point.y > high.y {
                    *high = point;
                }
            }
            _ => {
                if let Some((_, low, high)) = current {
                    keep(&mut kept, low, high);
                }
                current = Some((column, point, point));
            }
        }
    }
    if let Some((_, low, high)) = current {
        keep(&mut kept, low, high);
    }
    PlotPoints::Owned(kept)
}

/// Pushes one column's extremes in x order, once if they are the same point.
fn keep(kept: &mut Vec<PlotPoint>, low: PlotPoint, high: PlotPoint) {
    if low == high {
        kept.push(low);
    } else if low.x <= high.x {
        kept.extend([low, high]);
    } else {
        kept.extend([high, low]);
    }
}

#[cfg(test)]
mod tests {
    use egui_plot::{PlotPoint, PlotPoints};

    use super::visible;

    fn ramp(len: usize) -> Vec<PlotPoint> {
        (0..len)
            .map(|i| PlotPoint::new(i as f64, (i % 7) as f64))
            .collect()
    }

    #[test]
    fn few_points_are_borrowed_whole() {
        let points = ramp(10);
        let PlotPoints::Borrowed(kept) = visible(&points, 0.0..=9.0, 100)
        else {
            panic!("expected borrowed points");
        };
        assert_eq!(kept, points.as_slice());
    }

    #[test]
    fn range_keeps_one_point_beyond_each_end() {
        let points = ramp(100);
        let PlotPoints::Borrowed(kept) = visible(&points, 10.5..=20.5, 100)
        else {
            panic!("expected borrowed points");
        };
        assert_eq!(kept.first().unwrap().x, 10.0);
        assert_eq!(kept.last().unwrap().x, 21.0);
    }

    #[test]
    fn decimation_bounds_the_count_and_keeps_a_spike() {
        let mut points = ramp(100_000);
        points[54_321].y = 1000.0;
        points[12_345].y = -1000.0;
        let kept = visible(&points, 0.0..=99_999.0, 200);
        let kept = kept.points();
        assert!(kept.len() <= 2 * 200 + 4, "{} points", kept.len());
        assert!(kept.iter().any(|point| point.y == 1000.0));
        assert!(kept.iter().any(|point| point.y == -1000.0));
        assert!(kept.windows(2).all(|pair| pair[0].x <= pair[1].x));
    }
}
