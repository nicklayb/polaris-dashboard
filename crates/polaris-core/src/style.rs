//! Semantic styling shared by every renderer.
//!
//! Widgets never reference concrete colors; they resolve a [`Level`] that
//! each renderer maps to its own palette.

use serde::Deserialize;

/// Severity of a value, mapped to a color by renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    #[default]
    Neutral,
    Ok,
    Warning,
    Critical,
}

/// A threshold step: values greater than or equal to `value` take `level`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Threshold {
    pub value: f64,
    pub level: Level,
}

/// Replaces an exact numeric value with a text and a level (e.g. `1` → `UP`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ValueMapping {
    pub value: f64,
    pub text: String,
    #[serde(default)]
    pub level: Level,
}

/// Returns the level of the highest threshold reached by `value`.
///
/// Thresholds may be declared in any order. Returns [`Level::Neutral`] when
/// no threshold is reached.
pub fn level_for(value: f64, thresholds: &[Threshold]) -> Level {
    thresholds
        .iter()
        .filter(|threshold| value >= threshold.value)
        .max_by(|left_threshold, right_threshold| {
            left_threshold.value.total_cmp(&right_threshold.value)
        })
        .map_or(Level::Neutral, |threshold| threshold.level)
}

/// Returns the mapping matching `value` exactly, if any.
pub fn mapping_for(value: f64, mappings: &[ValueMapping]) -> Option<&ValueMapping> {
    mappings.iter().find(|mapping| mapping.value == value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temperature_thresholds() -> Vec<Threshold> {
        vec![
            Threshold {
                value: 85.0,
                level: Level::Critical,
            },
            Threshold {
                value: 0.0,
                level: Level::Ok,
            },
            Threshold {
                value: 70.0,
                level: Level::Warning,
            },
        ]
    }

    #[test]
    fn level_for_picks_highest_reached_threshold() {
        let thresholds = temperature_thresholds();

        assert_eq!(level_for(42.0, &thresholds), Level::Ok);
        assert_eq!(level_for(70.0, &thresholds), Level::Warning);
        assert_eq!(level_for(99.0, &thresholds), Level::Critical);
    }

    #[test]
    fn level_for_is_neutral_below_every_threshold() {
        assert_eq!(level_for(-5.0, &temperature_thresholds()), Level::Neutral);
        assert_eq!(level_for(12.0, &[]), Level::Neutral);
    }

    #[test]
    fn mapping_for_matches_exact_value() {
        let mappings = vec![
            ValueMapping {
                value: 1.0,
                text: "Springfield Nuclear Plant online".into(),
                level: Level::Ok,
            },
            ValueMapping {
                value: 0.0,
                text: "Meltdown".into(),
                level: Level::Critical,
            },
        ];

        assert_eq!(
            mapping_for(0.0, &mappings).map(|mapping| mapping.text.as_str()),
            Some("Meltdown")
        );
        assert!(mapping_for(0.5, &mappings).is_none());
    }
}
