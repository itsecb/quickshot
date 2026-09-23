//! Alert rules for "Watch a region" (pure, unit-tested): what counts as a change, and text
//! that appears or goes away.

use regex::RegexBuilder;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Condition {
    /// Alert when at least this share of the region (0-100) changed.
    Change { min_percent: f64 },
    /// Alert when the text (substring, or `re:<regex>`) shows up.
    TextAppears { pattern: String },
    /// Alert when the text goes away.
    TextGone { pattern: String },
}

impl Default for Condition {
    fn default() -> Self {
        Condition::Change { min_percent: 0.5 }
    }
}

/// What one check saw.
pub struct Observation<'a> {
    pub changed_percent: f64,
    pub has_boxes: bool,
    /// OCR text, only computed for text conditions.
    pub text: Option<&'a str>,
}

/// Case-insensitive substring, or a regular expression after `re:`.
pub fn text_matches(pattern: &str, text: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    match pattern.strip_prefix("re:") {
        Some(re) => RegexBuilder::new(re)
            .case_insensitive(true)
            .build()
            .map(|r| r.is_match(text))
            .unwrap_or(false),
        None => text.to_lowercase().contains(&pattern.to_lowercase()),
    }
}

/// Decide whether to alert. `was_present` is the text state after the previous check
/// (text conditions alert on the transition, not on every check). Returns (alert, is_present).
pub fn should_alert(cond: &Condition, was_present: bool, obs: &Observation) -> (bool, bool) {
    match cond {
        Condition::Change { min_percent } => {
            (obs.has_boxes && obs.changed_percent >= *min_percent, false)
        }
        Condition::TextAppears { pattern } => {
            let now = obs.text.is_some_and(|t| text_matches(pattern, t));
            (now && !was_present, now)
        }
        Condition::TextGone { pattern } => {
            let now = obs.text.is_some_and(|t| text_matches(pattern, t));
            (!now && was_present, now)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(p: f64, boxes: bool, text: Option<&str>) -> Observation<'_> {
        Observation {
            changed_percent: p,
            has_boxes: boxes,
            text,
        }
    }

    #[test]
    fn change_needs_real_boxes_and_enough_area() {
        let c = Condition::Change { min_percent: 0.5 };
        assert!(should_alert(&c, false, &obs(3.0, true, None)).0);
        assert!(!should_alert(&c, false, &obs(0.2, true, None)).0);
        assert!(!should_alert(&c, false, &obs(3.0, false, None)).0);
    }

    #[test]
    fn text_alerts_on_transitions_only() {
        let c = Condition::TextAppears {
            pattern: "failed".into(),
        };
        assert_eq!(
            should_alert(&c, false, &obs(0.0, false, Some("Job FAILED"))),
            (true, true)
        );
        // still there on the next check: no repeat alert
        assert_eq!(
            should_alert(&c, true, &obs(0.0, false, Some("Job failed"))),
            (false, true)
        );
        assert_eq!(
            should_alert(&c, true, &obs(0.0, false, Some("Job ok"))),
            (false, false)
        );
        let g = Condition::TextGone {
            pattern: "re:^running".into(),
        };
        assert_eq!(
            should_alert(&g, true, &obs(0.0, false, Some("Stopped"))),
            (true, false)
        );
        assert_eq!(
            should_alert(&g, false, &obs(0.0, false, Some("Stopped"))),
            (false, false)
        );
    }

    #[test]
    fn text_matching() {
        assert!(text_matches("error", "An ERROR occurred"));
        assert!(text_matches(
            "re:\\b5\\d\\d\\b",
            "HTTP 503 Service Unavailable"
        ));
        assert!(!text_matches("re:(", "anything"));
        assert!(!text_matches("  ", "anything"));
    }

    #[test]
    fn conditions_round_trip_as_tagged_json() {
        let c = Condition::TextAppears {
            pattern: "x".into(),
        };
        let j = serde_json::to_string(&c).unwrap();
        assert_eq!(j, r#"{"kind":"textAppears","pattern":"x"}"#);
        assert_eq!(serde_json::from_str::<Condition>(&j).unwrap(), c);
        let m: Condition = serde_json::from_str(r#"{"kind":"change","minPercent":2.0}"#).unwrap();
        assert_eq!(m, Condition::Change { min_percent: 2.0 });
    }
}
