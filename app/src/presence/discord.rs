//! Connection to the local Discord client via IPC.
//!
//! Responsibilities: keeping the connection alive (Discord might not be
//! running, or might restart at any moment), respecting the update rate
//! limit, and not spending sends on states that haven't changed.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use tracing::{debug, info, warn};

use super::spec::ActivitySpec;

/// Discord limits activity updates to one every 15 seconds.
pub const MIN_UPDATE_INTERVAL: Duration = Duration::from_secs(15);

const BACKOFF_INITIAL: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// Current Unix instant in seconds.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub struct DiscordSink {
    client: DiscordIpcClient,
    connected: bool,
    /// Current wait between connection retries.
    backoff: Duration,
    /// Moment from which a reconnection attempt can be made again.
    retry_after: Option<Instant>,
    last_sent_at: Option<Instant>,
    last_spec: Option<ActivitySpec>,
}

impl DiscordSink {
    pub fn new(application_id: &str) -> Result<Self> {
        // `DiscordIpcClient::new` only stores the id; it doesn't open anything yet.
        let client = DiscordIpcClient::new(application_id);

        Ok(Self {
            client,
            connected: false,
            backoff: BACKOFF_INITIAL,
            retry_after: None,
            last_sent_at: None,
            last_spec: None,
        })
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Attempts to connect if it's time. It's not an error for Discord to be
    /// closed: it retries later with increasing backoff.
    ///
    /// Returns `true` if there is a usable connection.
    pub fn ensure_connected(&mut self) -> bool {
        if self.connected {
            return true;
        }
        if let Some(retry_after) = self.retry_after {
            if Instant::now() < retry_after {
                return false;
            }
        }

        match self.client.connect() {
            Ok(()) => {
                info!("connected to Discord's IPC");
                self.connected = true;
                self.backoff = BACKOFF_INITIAL;
                self.retry_after = None;
                // A reconnection discards the state Discord had: it must be
                // resent even if the content hasn't changed.
                self.last_spec = None;
                self.last_sent_at = None;
                true
            }
            Err(err) => {
                debug!(%err, retry_in = ?self.backoff, "Discord not available");
                self.retry_after = Some(Instant::now() + self.backoff);
                self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
                false
            }
        }
    }

    /// Publishes the state if it has changed and if the time limit allows it.
    ///
    /// Returns `true` if something was sent.
    pub fn publish(&mut self, spec: &ActivitySpec) -> bool {
        if !self.ensure_connected() {
            return false;
        }

        if let Some(previous) = &self.last_spec {
            if !spec.differs_from(previous) {
                return false;
            }
        }

        if let Some(sent_at) = self.last_sent_at {
            if sent_at.elapsed() < MIN_UPDATE_INTERVAL {
                debug!("change pending: still within the 15 s limit");
                return false;
            }
        }

        match self.client.set_activity(spec.to_activity()) {
            Ok(()) => {
                debug!(?spec, "activity published");
                self.last_spec = Some(spec.clone());
                self.last_sent_at = Some(Instant::now());
                true
            }
            Err(err) => {
                warn!(%err, "publish failed; will be marked for reconnection");
                self.drop_connection();
                false
            }
        }
    }

    /// Clears the activity (Factorio is no longer running).
    ///
    /// Used by the phase 4 main loop, when the `process` source
    /// stops seeing `factorio.exe`.
    #[allow(dead_code, reason = "the main loop lands in phase 4")]
    pub fn clear(&mut self) {
        if !self.connected {
            return;
        }
        // Nothing to clear if we never got to publish.
        if self.last_spec.is_none() {
            return;
        }
        match self.client.clear_activity() {
            Ok(()) => {
                debug!("activity cleared");
                self.last_spec = None;
                self.last_sent_at = None;
            }
            Err(err) => {
                warn!(%err, "failed to clear the activity");
                self.drop_connection();
            }
        }
    }

    fn drop_connection(&mut self) {
        self.connected = false;
        self.retry_after = Some(Instant::now() + self.backoff);
        self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
        self.last_spec = None;
        self.last_sent_at = None;
    }
}

impl Drop for DiscordSink {
    fn drop(&mut self) {
        if self.connected {
            let _ = self.client.close();
        }
    }
}
