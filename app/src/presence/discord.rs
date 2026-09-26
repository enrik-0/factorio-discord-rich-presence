//! Conexión con el cliente local de Discord vía IPC.
//!
//! Responsabilidades: mantener la conexión viva (Discord puede no estar
//! arrancado, o reiniciarse en cualquier momento), respetar el límite de
//! actualizaciones y no gastar envíos en estados que no han cambiado.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use tracing::{debug, info, warn};

use super::spec::ActivitySpec;

/// Discord limita las actualizaciones de actividad a una cada 15 segundos.
pub const MIN_UPDATE_INTERVAL: Duration = Duration::from_secs(15);

const BACKOFF_INITIAL: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// Instante Unix actual en segundos.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub struct DiscordSink {
    client: DiscordIpcClient,
    connected: bool,
    /// Espera actual entre reintentos de conexión.
    backoff: Duration,
    /// Momento a partir del cual se puede volver a intentar conectar.
    retry_after: Option<Instant>,
    last_sent_at: Option<Instant>,
    last_spec: Option<ActivitySpec>,
}

impl DiscordSink {
    pub fn new(application_id: &str) -> Result<Self> {
        // `DiscordIpcClient::new` sólo guarda el id; no abre nada todavía.
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

    /// Intenta conectar si toca. No es un error que Discord esté cerrado: se
    /// reintenta más tarde con espera creciente.
    ///
    /// Devuelve `true` si hay conexión utilizable.
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
                info!("conectado al IPC de Discord");
                self.connected = true;
                self.backoff = BACKOFF_INITIAL;
                self.retry_after = None;
                // Una reconexión descarta el estado que Discord tenía: hay que
                // reenviar aunque el contenido no haya cambiado.
                self.last_spec = None;
                self.last_sent_at = None;
                true
            }
            Err(err) => {
                debug!(%err, reintento_en = ?self.backoff, "Discord no disponible");
                self.retry_after = Some(Instant::now() + self.backoff);
                self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
                false
            }
        }
    }

    /// Publica el estado si ha cambiado y si el límite de tiempo lo permite.
    ///
    /// Devuelve `true` si se envió algo.
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
                debug!("cambio pendiente: aún dentro del límite de 15 s");
                return false;
            }
        }

        match self.client.set_activity(spec.to_activity()) {
            Ok(()) => {
                debug!(?spec, "actividad publicada");
                self.last_spec = Some(spec.clone());
                self.last_sent_at = Some(Instant::now());
                true
            }
            Err(err) => {
                warn!(%err, "fallo al publicar; se marcará para reconectar");
                self.drop_connection();
                false
            }
        }
    }

    /// Borra la actividad (Factorio ya no está en ejecución).
    ///
    /// La usa el bucle principal de la fase 4, cuando la fuente `process`
    /// deja de ver `factorio.exe`.
    #[allow(dead_code, reason = "el bucle principal llega en la fase 4")]
    pub fn clear(&mut self) {
        if !self.connected {
            return;
        }
        // Nada que borrar si nunca llegamos a publicar.
        if self.last_spec.is_none() {
            return;
        }
        match self.client.clear_activity() {
            Ok(()) => {
                debug!("actividad borrada");
                self.last_spec = None;
                self.last_sent_at = None;
            }
            Err(err) => {
                warn!(%err, "fallo al borrar la actividad");
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
