//! Per-app capture rules: e.g. never keep password-manager captures in history, always
//! auto-redact the admin console, send captures of a dashboard to a fixed folder.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Rule {
    pub enabled: bool,
    pub name: String,
    /// Comma-separated, case-insensitive substrings of the app name ("KeePass, Bitwarden").
    pub app: String,
    /// Comma-separated, case-insensitive substrings of the window title.
    pub title: String,
    pub skip_history: bool,
    pub auto_redact: bool,
    pub auto_copy: bool,
    /// Also save every matching capture to this folder.
    pub save_dir: Option<String>,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            enabled: true,
            name: String::new(),
            app: String::new(),
            title: String::new(),
            skip_history: false,
            auto_redact: false,
            auto_copy: false,
            save_dir: None,
        }
    }
}

/// Shipped enabled: captures of password managers never land in history.
pub fn default_rules() -> Vec<Rule> {
    vec![Rule {
        name: "Password managers".into(),
        app: "KeePass, Bitwarden, 1Password, LastPass, Keeper, Dashlane, Password Safe, RoboForm"
            .into(),
        skip_history: true,
        ..Default::default()
    }]
}

fn matches_any(list: &str, value: &str) -> bool {
    let value = value.to_lowercase();
    list.split(',')
        .map(|t| t.trim().to_lowercase())
        .any(|t| !t.is_empty() && value.contains(&t))
}

impl Rule {
    /// Every non-empty condition must match, and at least one condition must be set.
    pub fn matches(&self, app: &str, title: &str) -> bool {
        let has_app = !self.app.trim().is_empty();
        let has_title = !self.title.trim().is_empty();
        self.enabled
            && (has_app || has_title)
            && (!has_app || matches_any(&self.app, app))
            && (!has_title || matches_any(&self.title, title))
    }
}

/// All enabled rules that match, merged: any rule can switch a behaviour on; the first
/// matching rule with a folder decides where to save.
pub fn effective(rules: &[Rule], app: &str, title: &str) -> Option<Rule> {
    let mut hits = rules.iter().filter(|r| r.matches(app, title)).peekable();
    hits.peek()?;
    let mut out = Rule {
        name: String::new(),
        ..Default::default()
    };
    let mut names = Vec::new();
    for r in hits {
        names.push(r.name.clone());
        out.skip_history |= r.skip_history;
        out.auto_redact |= r.auto_redact;
        out.auto_copy |= r.auto_copy;
        if out.save_dir.is_none() {
            out.save_dir = r.save_dir.clone().filter(|d| !d.trim().is_empty());
        }
    }
    out.name = names.join(", ");
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(app: &str, title: &str) -> Rule {
        Rule {
            name: format!("{app}/{title}"),
            app: app.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    #[test]
    fn default_rule_catches_password_managers() {
        let rules = default_rules();
        let hit = effective(&rules, "KeePassXC", "Database.kdbx - KeePassXC").unwrap();
        assert!(hit.skip_history);
        assert!(effective(&rules, "bitwarden", "Vault").is_some());
        assert!(effective(&rules, "Google Chrome", "Inbox").is_none());
    }

    #[test]
    fn all_set_conditions_must_match() {
        let r = rule("Chrome", "Grafana");
        assert!(r.matches("Google Chrome", "Grafana - Dashboards"));
        assert!(!r.matches("Google Chrome", "Inbox"));
        assert!(!r.matches("Firefox", "Grafana"));
        // a rule with no conditions never matches everything by accident
        assert!(!rule("", " ").matches("anything", "at all"));
    }

    #[test]
    fn disabled_rules_are_ignored() {
        let mut r = rule("mmc", "");
        r.enabled = false;
        assert!(effective(&[r], "mmc", "Event Viewer").is_none());
    }

    #[test]
    fn matching_rules_merge() {
        let mut a = rule("Chrome", "");
        a.auto_copy = true;
        let mut b = rule("", "Azure");
        b.auto_redact = true;
        b.save_dir = Some("D:\\evidence".into());
        let mut c = rule("Chrome", "Azure");
        c.save_dir = Some("C:\\other".into());
        let hit = effective(&[a, b, c], "Google Chrome", "Azure Portal").unwrap();
        assert!(hit.auto_copy && hit.auto_redact && !hit.skip_history);
        assert_eq!(hit.save_dir.as_deref(), Some("D:\\evidence"));
    }
}
