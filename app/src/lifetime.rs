//! Cuándo debe cerrarse la aplicación en modo lanzador.
//!
//! En modo lanzador la aplicación nace con el juego y muere con él. No se ata al
//! proceso hijo que ella misma arranca: cuando Factorio se lanza fuera de Steam,
//! el primer `factorio.exe` sale enseguida y Steam lo relanza con otro PID. Por
//! eso la decisión se toma sobre "¿hay algún factorio.exe?", con un margen.

use std::time::{Duration, Instant};

/// Margen desde que desaparece Factorio hasta cerrar. Cubre el relanzamiento de
/// Steam y los cierres que dejan el proceso vivo unos segundos.
pub const GRACE: Duration = Duration::from_secs(10);

/// Espera máxima a que Factorio aparezca. Pasado este tiempo sin verlo nunca,
/// el arranque ha fallado y no tiene sentido quedarse residente.
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

    /// ¿Toca cerrar? Se llama en cada sondeo con el estado actual del proceso.
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
    fn mientras_el_juego_corre_no_se_cierra() {
        let t0 = Instant::now();
        let mut life = GameLifetime::new(t0);
        assert!(!life.should_exit(true, t0));
        assert!(!life.should_exit(true, t0 + secs(3600)));
    }

    #[test]
    fn al_cerrarse_el_juego_espera_el_margen_antes_de_salir() {
        let t0 = Instant::now();
        let mut life = GameLifetime::new(t0);
        assert!(!life.should_exit(true, t0));

        assert!(!life.should_exit(false, t0 + secs(100)));
        assert!(!life.should_exit(false, t0 + secs(100) + GRACE - secs(1)));
        assert!(life.should_exit(false, t0 + secs(100) + GRACE));
    }

    #[test]
    fn el_relanzamiento_de_steam_no_cierra_la_aplicacion() {
        // El primer proceso desaparece y otro nuevo aparece dentro del margen.
        let t0 = Instant::now();
        let mut life = GameLifetime::new(t0);
        assert!(!life.should_exit(true, t0));
        assert!(!life.should_exit(false, t0 + secs(2)));
        assert!(!life.should_exit(true, t0 + secs(4)));

        // El margen se reinicia: una nueva ausencia cuenta desde cero.
        assert!(!life.should_exit(false, t0 + secs(5)));
        assert!(!life.should_exit(false, t0 + secs(5) + GRACE - secs(1)));
        assert!(life.should_exit(false, t0 + secs(5) + GRACE));
    }

    #[test]
    fn si_el_juego_nunca_aparece_se_rinde_al_agotar_el_plazo() {
        let t0 = Instant::now();
        let mut life = GameLifetime::new(t0);
        assert!(!life.should_exit(false, t0 + STARTUP_TIMEOUT - secs(1)));
        assert!(life.should_exit(false, t0 + STARTUP_TIMEOUT));
    }

    #[test]
    fn el_plazo_de_arranque_no_aplica_si_el_juego_ya_se_vio() {
        // Un juego visto y cerrado usa el margen corto, muy por debajo del plazo
        // de arranque de 90 s.
        let t0 = Instant::now();
        let mut life = GameLifetime::new(t0);
        assert!(!life.should_exit(true, t0 + secs(1)));
        assert!(!life.should_exit(false, t0 + secs(2)));
        assert!(life.should_exit(false, t0 + secs(2) + GRACE));
        assert!(GRACE < STARTUP_TIMEOUT);
    }
}
