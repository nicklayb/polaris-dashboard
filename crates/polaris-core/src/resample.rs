//! Fits time series points into a fixed number of slots, for renderers
//! drawing one value per cell.

/// Splits `start..end` into `bucket_count` equal buckets and averages the
/// points falling in each one. Buckets without points are `None`.
///
/// Points are `(unix seconds, value)`; non-finite values and points outside
/// the window are ignored.
pub fn bucket_averages(
    points: &[(f64, f64)],
    start: f64,
    end: f64,
    bucket_count: usize,
) -> Vec<Option<f64>> {
    let mut sums = vec![0.0; bucket_count];
    let mut counts = vec![0_u32; bucket_count];
    let span = end - start;
    if bucket_count == 0 || span <= 0.0 {
        return vec![None; bucket_count];
    }
    for (time, value) in points
        .iter()
        .filter(|(time, value)| value.is_finite() && *time >= start && *time <= end)
    {
        let index = (((time - start) / span) * bucket_count as f64) as usize;
        let index = index.min(bucket_count - 1);
        sums[index] += value;
        counts[index] += 1;
    }
    sums.into_iter()
        .zip(counts)
        .map(|(sum, count)| (count > 0).then(|| sum / f64::from(count)))
        .collect()
}

/// Latest finite value of a series, if any.
pub fn latest_value(points: &[(f64, f64)]) -> Option<f64> {
    points
        .iter()
        .rev()
        .map(|(_, value)| *value)
        .find(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn averages_points_per_bucket() {
        let donuts_eaten = [(0.0, 1.0), (1.0, 3.0), (5.0, 10.0), (9.9, 4.0)];

        assert_eq!(
            bucket_averages(&donuts_eaten, 0.0, 10.0, 2),
            [Some(2.0), Some(7.0)]
        );
    }

    #[test]
    fn leaves_empty_buckets_and_ignores_outsiders() {
        let reactor_readings = [(-5.0, 99.0), (1.0, 2.0), (2.0, f64::NAN), (12.0, 99.0)];

        assert_eq!(
            bucket_averages(&reactor_readings, 0.0, 10.0, 4),
            [Some(2.0), None, None, None]
        );
    }

    #[test]
    fn last_point_lands_in_last_bucket() {
        assert_eq!(
            bucket_averages(&[(10.0, 8.0)], 0.0, 10.0, 5),
            [None, None, None, None, Some(8.0)]
        );
    }

    #[test]
    fn handles_degenerate_windows() {
        assert_eq!(bucket_averages(&[(1.0, 1.0)], 5.0, 5.0, 2), [None, None]);
        assert!(bucket_averages(&[(1.0, 1.0)], 0.0, 5.0, 0).is_empty());
    }

    #[test]
    fn latest_value_skips_missing_samples() {
        assert_eq!(latest_value(&[(1.0, 42.0), (2.0, f64::NAN)]), Some(42.0));
        assert_eq!(latest_value(&[]), None);
    }
}
