//! When the application should close in launcher mode.
//!
//! In launcher mode the application is born with the game and dies with it.
//! It doesn't attach to the child process it starts itself: when Factorio is
//! launched outside of Steam, the first `factorio.exe` exits right away and
//! Steam relaunches it with a different PID. That's why the decision is based
//! on "is there any factorio.exe?", with a grace margin.

use std::time::{Duration, Instant};

/// Grace period from when Factorio disappears until closing. Covers Steam's
/// relaunch and shutdowns that leave the process alive for a few seconds.
pub const GRACE: Duration = Duration::from_secs(10);

/// Maximum wait for Factorio to appear. After this time without ever seeing
/// it, startup has failed and there's no point staying resident.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);

pub struct GameLifetime {
    started: Instant,
    seen: bool,
    gone_since: Option<Instant>,
}

impl GameLifetime {
    pub fn new(now: Instant) -> Self {
        Self {
            started: now,
            seen: false,
            gone_since: None,
        }
    }

    /// Time to close? Called on every poll with the process's current state.
    pub fn should_exit(&mut self, running: bool, now: Instant) -> bool {
        if running {
            self.seen = true;
            self.gone_since = None;
            return false;
        }

        if self.seen {
            let gone_since = *self.gone_since.get_or_insert(now);
            return now.duration_since(gone_since) >= GRACE;
        }

        now.duration_since(self.started) >= STARTUP_TIMEOUT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn while_the_game_is_running_it_does_not_close() {
        let t0 = Instant::now();
        let mut life = GameLifetime::new(t0);
        assert!(!life.should_exit(true, t0));
        assert!(!life.should_exit(true, t0 + secs(3600)));
    }

    #[test]
    fn when_the_game_closes_it_waits_the_grace_period_before_exiting() {
        let t0 = Instant::now();
        let mut life = GameLifetime::new(t0);
        assert!(!life.should_exit(true, t0));

        assert!(!life.should_exit(false, t0 + secs(100)));
        assert!(!life.should_exit(false, t0 + secs(100) + GRACE - secs(1)));
        assert!(life.should_exit(false, t0 + secs(100) + GRACE));
    }

    #[test]
    fn steams_relaunch_does_not_close_the_application() {
        // The first process disappears and a new one appears within the grace period.
        let t0 = Instant::now();
        let mut life = GameLifetime::new(t0);
        assert!(!life.should_exit(true, t0));
        assert!(!life.should_exit(false, t0 + secs(2)));
        assert!(!life.should_exit(true, t0 + secs(4)));

        // The grace period resets: a new absence counts from zero.
        assert!(!life.should_exit(false, t0 + secs(5)));
        assert!(!life.should_exit(false, t0 + secs(5) + GRACE - secs(1)));
        assert!(life.should_exit(false, t0 + secs(5) + GRACE));
    }

    #[test]
    fn if_the_game_never_appears_it_gives_up_once_the_timeout_elapses() {
        let t0 = Instant::now();
        let mut life = GameLifetime::new(t0);
        assert!(!life.should_exit(false, t0 + STARTUP_TIMEOUT - secs(1)));
        assert!(life.should_exit(false, t0 + STARTUP_TIMEOUT));
    }

    #[test]
    fn the_startup_timeout_does_not_apply_if_the_game_was_already_seen() {
        // A game that was seen and then closed uses the short grace period,
        // well under the 90 s startup timeout.
        let t0 = Instant::now();
        let mut life = GameLifetime::new(t0);
        assert!(!life.should_exit(true, t0 + secs(1)));
        assert!(!life.should_exit(false, t0 + secs(2)));
        assert!(life.should_exit(false, t0 + secs(2) + GRACE));
        assert!(GRACE < STARTUP_TIMEOUT);
    }
}
