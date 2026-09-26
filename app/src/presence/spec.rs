//! Representación propia y comparable de una actividad de Discord.
//!
//! El tipo `Activity` del crate usa `Cow<'a, str>`, lo que complica guardarlo para
//! comparar contra el siguiente. `ActivitySpec` es dueño de sus datos, implementa
//! `PartialEq` y se convierte a `Activity` justo antes de enviar.

use discord_rich_presence::activity::{Activity, Assets, Party, Timestamps};

/// Discord trunca los textos largos; recortamos nosotros para controlar dónde se corta.
#[allow(
    dead_code,
    reason = "lo consume el renderer de plantillas en la fase 3"
)]
pub const MAX_TEXT_LEN: usize = 128;

/// Margen de tolerancia al comparar marcas de tiempo, en segundos.
///
/// El inicio del cronómetro se recalcula en cada actualización como
/// `ahora − tiempo_jugado`, así que oscila uno o dos segundos aunque no haya
/// pasado nada. Sin esta tolerancia el deduplicador no detectaría nunca dos
/// estados iguales y gastaríamos el límite de una actualización cada 15 s.
const TIMESTAMP_TOLERANCE_SECS: i64 = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivitySpec {
    pub details: Option<String>,
    pub state: Option<String>,
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
    /// Instante Unix (segundos) en que arrancó el cronómetro.
    pub start_timestamp: Option<i64>,
    /// (actual, máximo) de jugadores en la partida.
    pub party: Option<(i32, i32)>,
}

impl ActivitySpec {
    /// ¿Merece la pena gastar una actualización en enviar este estado?
    ///
    /// Todo se compara por igualdad exacta salvo el cronómetro, que tolera
    /// [`TIMESTAMP_TOLERANCE_SECS`] de deriva.
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

    /// Construye el `Activity` del crate tomando prestados los datos de `self`.
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

/// Recorta a [`MAX_TEXT_LEN`] respetando límites de carácter UTF-8.
///
/// Importante para nombres de save y de tecnologías traducidas, que pueden
/// contener acentos y caracteres multibyte.
#[allow(
    dead_code,
    reason = "lo consume el renderer de plantillas en la fase 3"
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
    fn estado_identico_no_se_reenvia() {
        assert!(!base().differs_from(&base()));
    }

    #[test]
    fn deriva_pequena_del_cronometro_no_cuenta_como_cambio() {
        let mut drifted = base();
        drifted.start_timestamp = Some(1_000_003);
        assert!(!drifted.differs_from(&base()));
    }

    #[test]
    fn salto_grande_del_cronometro_si_cuenta() {
        let mut reloaded = base();
        reloaded.start_timestamp = Some(1_000_600);
        assert!(reloaded.differs_from(&base()));
    }

    #[test]
    fn cambio_de_texto_cuenta() {
        let mut other = base();
        other.state = Some("Investigando Robótica (10%)".into());
        assert!(other.differs_from(&base()));
    }

    #[test]
    fn truncado_respeta_caracteres_multibyte() {
        let long = "á".repeat(200);
        let result = truncate(&long);
        assert_eq!(result.chars().count(), MAX_TEXT_LEN);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncado_deja_intacto_lo_corto() {
        assert_eq!(truncate("Fulgora"), "Fulgora");
    }
}
