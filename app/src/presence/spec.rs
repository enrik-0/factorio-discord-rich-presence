//! Our own, comparable representation of a Discord activity.
//!
//! The crate's `Activity` type uses `Cow<'a, str>`, which makes it awkward to keep
//! around for comparing against the next one. `ActivitySpec` owns its data, implements
//! `PartialEq`, and converts to `Activity` right before sending.

use discord_rich_presence::activity::{Activity, Assets, Party, Timestamps};

/// Discord truncates long texts; we trim it ourselves to control where it gets cut.
#[allow(
    dead_code,
    reason = "consumed by the template renderer in phase 3"
)]
pub const MAX_TEXT_LEN: usize = 128;

/// Tolerance margin when comparing timestamps, in seconds.
///
/// The timer's start is recalculated on every update as
/// `now − playtime`, so it drifts by one or two seconds even when nothing
/// has happened. Without this tolerance the deduplicator would never detect
/// two equal states, and we'd burn through the one-update-per-15s limit.
const TIMESTAMP_TOLERANCE_SECS: i64 = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivitySpec {
    pub details: Option<String>,
    pub state: Option<String>,
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
    /// Unix instant (seconds) at which the timer started.
    pub start_timestamp: Option<i64>,
    /// (current, max) players in the game.
    pub party: Option<(i32, i32)>,
}

impl ActivitySpec {
    /// Is it worth spending an update to send this state?
    ///
    /// Everything is compared by exact equality except the timer, which tolerates
    /// [`TIMESTAMP_TOLERANCE_SECS`] of drift.
    pub fn differs_from(&self, previous: &ActivitySpec) -> bool {
        if self.details != previous.details
            || self.state != previous.state
            || self.large_image != previous.large_image
            || self.large_text != previous.large_text
            || self.small_image != previous.small_image
            || self.small_text != previous.small_text
            || self.party != previous.party
        {
            return true;
        }

        match (self.start_timestamp, previous.start_timestamp) {
            (Some(a), Some(b)) => (a - b).abs() > TIMESTAMP_TOLERANCE_SECS,
            (None, None) => false,
            _ => true,
        }
    }

    /// Builds the crate's `Activity` by borrowing the data from `self`.
    pub fn to_activity(&self) -> Activity<'_> {
        let mut activity = Activity::new();

        if let Some(details) = &self.details {
            activity = activity.details(details.as_str());
        }
        if let Some(state) = &self.state {
            activity = activity.state(state.as_str());
        }

        let mut assets = Assets::new();
        let mut has_assets = false;
        if let Some(image) = &self.large_image {
            assets = assets.large_image(image.as_str());
            has_assets = true;
        }
        if let Some(text) = &self.large_text {
            assets = assets.large_text(text.as_str());
            has_assets = true;
        }
        if let Some(image) = &self.small_image {
            assets = assets.small_image(image.as_str());
            has_assets = true;
        }
        if let Some(text) = &self.small_text {
            assets = assets.small_text(text.as_str());
            has_assets = true;
        }
        if has_assets {
            activity = activity.assets(assets);
        }

        if let Some(start) = self.start_timestamp {
            activity = activity.timestamps(Timestamps::new().start(start));
        }

        if let Some((current, max)) = self.party {
            activity = activity.party(Party::new().size([current, max]));
        }

        activity
    }
}

/// Truncates to [`MAX_TEXT_LEN`] respecting UTF-8 character boundaries.
///
/// Important for save names and translated technology names, which can
/// contain accents and multibyte characters.
#[allow(
    dead_code,
    reason = "consumed by the template renderer in phase 3"
)]
pub fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT_LEN {
        return text.to_string();
    }
    let truncated: String = text.chars().take(MAX_TEXT_LEN - 1).collect();
    format!("{}…", truncated.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ActivitySpec {
        ActivitySpec {
            details: Some("Fulgora · Cohetes S.A.".into()),
            state: Some("Investigando Planta electromagnética (64%)".into()),
            start_timestamp: Some(1_000_000),
            ..Default::default()
        }
    }

    #[test]
    fn identical_state_is_not_resent() {
        assert!(!base().differs_from(&base()));
    }

    #[test]
    fn small_timer_drift_does_not_count_as_a_change() {
        let mut drifted = base();
        drifted.start_timestamp = Some(1_000_003);
        assert!(!drifted.differs_from(&base()));
    }

    #[test]
    fn a_large_timer_jump_does_count() {
        let mut reloaded = base();
        reloaded.start_timestamp = Some(1_000_600);
        assert!(reloaded.differs_from(&base()));
    }

    #[test]
    fn a_text_change_counts() {
        let mut other = base();
        other.state = Some("Investigando Robótica (10%)".into());
        assert!(other.differs_from(&base()));
    }

    #[test]
    fn truncation_respects_multibyte_characters() {
        let long = "á".repeat(200);
        let result = truncate(&long);
        assert_eq!(result.chars().count(), MAX_TEXT_LEN);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncation_leaves_short_text_untouched() {
        assert_eq!(truncate("Fulgora"), "Fulgora");
    }
}
