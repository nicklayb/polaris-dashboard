//! Units and human readable value formatting.

use serde::Deserialize;

const BYTES_PER_KIBIBYTE: f64 = 1024.0;
const BYTE_UNIT_SUFFIXES: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
const PERCENT_PER_RATIO: f64 = 100.0;
const SECONDS_PER_MINUTE: f64 = 60.0;
const SECONDS_PER_HOUR: f64 = 3600.0;
const SECONDS_PER_DAY: f64 = 86400.0;

/// How a raw numeric value must be presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Plain number using the configured decimals.
    #[default]
    None,
    /// Integer count, never shows decimals.
    Count,
    /// Value already expressed in percent (0 to 100).
    Percent,
    /// Ratio between 0 and 1, displayed as percent.
    Ratio,
    /// Bytes, displayed with binary prefixes.
    Bytes,
    /// Degrees Celsius.
    Celsius,
    /// Seconds, displayed as the largest meaningful unit.
    Seconds,
}

/// Formats `value` according to `unit` with `decimals` fractional digits.
pub fn format_value(value: f64, unit: Unit, decimals: usize) -> String {
    if !value.is_finite() {
        return "NaN".to_string();
    }
    match unit {
        Unit::None => format!("{value:.decimals$}"),
        Unit::Count => format!("{value:.0}"),
        Unit::Percent => format!("{value:.decimals$}%"),
        Unit::Ratio => format!("{:.decimals$}%", value * PERCENT_PER_RATIO),
        Unit::Bytes => format_bytes(value, decimals),
        Unit::Celsius => format!("{value:.decimals$}°C"),
        Unit::Seconds => format_seconds(value),
    }
}

fn format_bytes(value: f64, decimals: usize) -> String {
    let mut scaled = value;
    let mut suffix_index = 0;
    while scaled.abs() >= BYTES_PER_KIBIBYTE && suffix_index < BYTE_UNIT_SUFFIXES.len() - 1 {
        scaled /= BYTES_PER_KIBIBYTE;
        suffix_index += 1;
    }
    format!("{scaled:.decimals$}{}", BYTE_UNIT_SUFFIXES[suffix_index])
}

fn format_seconds(value: f64) -> String {
    let scales = [
        (SECONDS_PER_DAY, "d"),
        (SECONDS_PER_HOUR, "h"),
        (SECONDS_PER_MINUTE, "m"),
    ];
    scales
        .iter()
        .find(|(seconds, _)| value.abs() >= *seconds)
        .map_or_else(
            || format!("{value:.0}s"),
            |(seconds, suffix)| format!("{:.0}{suffix}", value / seconds),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_ratio_as_percent() {
        assert_eq!(format_value(0.4242, Unit::Ratio, 1), "42.4%");
    }

    #[test]
    fn formats_bytes_with_binary_prefix() {
        assert_eq!(
            format_value(16.0 * 1024.0 * 1024.0 * 1024.0, Unit::Bytes, 1),
            "16.0GiB"
        );
        assert_eq!(format_value(742.0, Unit::Bytes, 0), "742B");
    }

    #[test]
    fn formats_count_without_decimals() {
        assert_eq!(format_value(5.0, Unit::Count, 2), "5");
    }

    #[test]
    fn formats_seconds_with_largest_unit() {
        assert_eq!(format_value(3.0 * 86400.0, Unit::Seconds, 0), "3d");
        assert_eq!(format_value(90.0, Unit::Seconds, 0), "2m");
        assert_eq!(format_value(12.0, Unit::Seconds, 0), "12s");
    }

    #[test]
    fn formats_celsius() {
        assert_eq!(format_value(65.0, Unit::Celsius, 0), "65°C");
    }
}
