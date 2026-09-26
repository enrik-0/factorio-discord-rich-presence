//! Estado compartido entre el hilo de vigilancia y el icono de la bandeja.
//!
//! Sin consola ni ventana, el icono es la única forma de saber si la aplicación
//! está viva y qué está haciendo. Por eso el hilo trabajador publica aquí lo que
//! ve, y el menú lo lee para el texto emergente.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub factorio_running: bool,
    pub discord_connected: bool,
    /// `false` cuando el mod no está y sólo tenemos el registro del juego.
    pub mod_data: bool,
    /// Lo que se está publicando ahora mismo, para verlo de un vistazo.
    pub headline: Option<String>,
}

impl Status {
    /// Texto emergente del icono. Debe caber en una línea y decir lo esencial:
    /// si falta Discord o falta Factorio, y qué se está publicando.
    pub fn tooltip(&self) -> String {
        let mut lines = vec!["Factorio Discord Rich Presence".to_string()];

        lines.push(match (self.factorio_running, self.discord_connected) {
            (false, _) => "Factorio no está en ejecución".into(),
            (true, false) => "Esperando a Discord…".into(),
            (true, true) if self.mod_data => "Publicando".into(),
            (true, true) => "Publicando (sin el mod)".into(),
        });

        if let Some(headline) = &self.headline {
            lines.push(headline.clone());
        }

        lines.join("\n")
    }
}

#[derive(Default)]
pub struct Shared {
    status: Mutex<Status>,
    shutdown: AtomicBool,
}

impl Shared {
    pub fn snapshot(&self) -> Status {
        // Un candado envenenado no debe tumbar la aplicación: lo que hay dentro
        // es informativo, no crítico.
        match self.status.lock() {
            Ok(status) => status.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub fn update(&self, apply: impl FnOnce(&mut Status)) {
        let mut guard = match self.status.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        apply(&mut guard);
    }

    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_texto_emergente_dice_que_falta() {
        let parado = Status::default();
        assert!(parado.tooltip().contains("Factorio no está en ejecución"));

        let sin_discord = Status {
            factorio_running: true,
            ..Default::default()
        };
        assert!(sin_discord.tooltip().contains("Esperando a Discord"));

        let degradado = Status {
            factorio_running: true,
            discord_connected: true,
            mod_data: false,
            headline: Some("claro".into()),
        };
        let texto = degradado.tooltip();
        assert!(texto.contains("sin el mod"));
        assert!(texto.contains("claro"));

        let completo = Status {
            mod_data: true,
            ..degradado
        };
        assert!(completo.tooltip().contains("Publicando"));
        assert!(!completo.tooltip().contains("sin el mod"));
    }

    #[test]
    fn el_cierre_se_pide_una_vez_y_persiste() {
        let shared = Shared::default();
        assert!(!shared.is_shutdown());
        shared.request_shutdown();
        assert!(shared.is_shutdown());
        assert!(shared.is_shutdown());
    }

    #[test]
    fn el_estado_se_actualiza_y_se_lee() {
        let shared = Shared::default();
        shared.update(|status| status.factorio_running = true);
        assert!(shared.snapshot().factorio_running);
    }
}
