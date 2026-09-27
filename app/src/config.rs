//! Carga de `config.toml`.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tracing::debug;

/// Permite probar sin tocar el fichero de configuración.
const ENV_APPLICATION_ID: &str = "FACTORIO_DRP_APP_ID";

/// Application ID de Discord incluido al compilar, para que quien instale la
/// aplicación no tenga que crear la suya. Lo inyecta el CI al construir el
/// instalador (`FACTORIO_DRP_DEFAULT_APP_ID`); en una compilación normal no existe
/// y el ID sale de `config.toml`. Es un dato público, pero así no vive en el
/// código fuente.
const INCLUDED_APPLICATION_ID: Option<&str> = option_env!("FACTORIO_DRP_DEFAULT_APP_ID");

/// Elige el ID a usar y comprueba que sea válido.
fn resolve_application_id<'a>(
    configured: &'a str,
    included: Option<&'static str>,
) -> Result<&'a str> {
    let configured = configured.trim();
    let id = if configured.is_empty() {
        included.map(str::trim).unwrap_or("")
    } else {
        configured
    };

    if id.is_empty() {
        bail!(
            "falta el Application ID de Discord.\n\
             Créalo en https://discord.com/developers/applications y ponlo en \
             config.toml (campo application_id) o en la variable {ENV_APPLICATION_ID}."
        );
    }
    if !id.chars().all(|c| c.is_ascii_digit()) {
        bail!("el Application ID debe ser sólo dígitos, recibido: {id:?}");
    }
    Ok(id)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Application ID de Discord. Obligatorio.
    pub application_id: String,
    /// Carpeta de datos de Factorio. Vacío = detección automática.
    pub factorio_data_dir: String,
    /// Clave del asset subido al portal de Discord. Vacío = sin imagen.
    pub large_image: String,
    pub privacy: Privacy,
    /// Modo avanzado. Ausente = reparto automático según los ajustes del mod.
    pub templates: Option<Templates>,
}

/// Reparto de los datos entre los pocos huecos que ofrece la tarjeta de Discord.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Templates {
    /// Primera línea.
    pub details: String,
    /// Segunda línea.
    pub state: String,
    /// Tooltip del icono.
    pub large_text: String,
    pub fallback: Fallback,
}

/// Plantillas de respaldo, para cuando la principal se queda sin datos.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Fallback {
    /// Se usa cuando no hay ninguna investigación en cola.
    pub state: String,
}

impl Default for Templates {
    fn default() -> Self {
        Self {
            details: "{planet} · {save}".into(),
            state: "Researching {research} ({research_pct}%)".into(),
            // Sin icono pequeño, el modo de juego se refugia en este tooltip.
            large_text: "{planet} · {tech_done}/{tech_total} technologies · {mode}".into(),
            fallback: Fallback::default(),
        }
    }
}

impl Default for Fallback {
    fn default() -> Self {
        Self {
            state: "{tech_done}/{tech_total} technologies".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Privacy {
    /// Nunca activo por defecto: expone la IP del servidor en el perfil público.
    pub share_server_address: bool,
    pub share_save_name: bool,
}

// Ni `Config` ni `Privacy` pueden derivar Default: arrancan con valores.
impl Default for Config {
    fn default() -> Self {
        Self {
            application_id: String::new(),
            factorio_data_dir: String::new(),
            large_image: "factorio".into(),
            privacy: Privacy::default(),
            templates: None,
        }
    }
}

impl Default for Privacy {
    fn default() -> Self {
        Self {
            share_server_address: false,
            share_save_name: true,
        }
    }
}

impl Config {
    /// Carga la configuración, buscando en orden:
    ///   1. la ruta explícita, si se pasó `--config`
    ///   2. `config.toml` junto al ejecutable
    ///   3. `%APPDATA%/factorio-discord-rp/config.toml`
    ///   4. `config.toml` en el directorio actual
    ///
    /// Si no aparece ninguno se usan los valores por defecto, de modo que la
    /// variable de entorno por sí sola baste para una prueba rápida.
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let path = match explicit {
            Some(path) => {
                if !path.exists() {
                    bail!("no existe el fichero de configuración {}", path.display());
                }
                Some(path.to_path_buf())
            }
            None => Self::discover(),
        };

        let mut config = match &path {
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .with_context(|| format!("no se pudo leer {}", path.display()))?;
                let config: Config = toml::from_str(&text)
                    .with_context(|| format!("{} no es un TOML válido", path.display()))?;
                debug!(ruta = %path.display(), "configuración cargada");
                config
            }
            None => {
                debug!("sin config.toml; usando valores por defecto");
                Config::default()
            }
        };

        if let Ok(id) = std::env::var(ENV_APPLICATION_ID) {
            if !id.trim().is_empty() {
                debug!("application_id tomado de {ENV_APPLICATION_ID}");
                config.application_id = id.trim().to_string();
            }
        }

        Ok(config)
    }

    fn discover() -> Option<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join(crate::paths::CONFIG_FILE));
            }
        }
        // Ubicación estable: con autoarranque el directorio actual es System32,
        // así que no se puede depender de él.
        if let Some(path) = crate::paths::config_in_app_dir() {
            candidates.push(path);
        }
        candidates.push(PathBuf::from(crate::paths::CONFIG_FILE));
        candidates.into_iter().find(|path| path.exists())
    }

    /// Valida lo imprescindible para poder hablar con Discord.
    ///
    /// Manda lo que haya en `config.toml` (o en la variable de entorno); si está
    /// vacío se usa el ID incluido al compilar el instalador.
    pub fn application_id(&self) -> Result<&str> {
        resolve_application_id(&self.application_id, INCLUDED_APPLICATION_ID)
    }

    /// Carpeta de datos de Factorio: la que contiene `script-output`, `saves`
    /// y `factorio-current.log`.
    pub fn factorio_data_dir(&self) -> Result<PathBuf> {
        if !self.factorio_data_dir.trim().is_empty() {
            let path = PathBuf::from(self.factorio_data_dir.trim());
            if !path.is_dir() {
                bail!("factorio_data_dir no es un directorio: {}", path.display());
            }
            return Ok(path);
        }

        let appdata = std::env::var("APPDATA")
            .context("no se pudo leer %APPDATA% para localizar Factorio")?;
        let path = PathBuf::from(appdata).join("Factorio");
        if !path.is_dir() {
            bail!(
                "no se encontró la carpeta de datos de Factorio en {}. \
                 Indícala a mano en config.toml (factorio_data_dir).",
                path.display()
            );
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rechaza_application_id_vacio() {
        // Sin valor incluido en la compilación: así no depende de cómo se compile.
        assert!(resolve_application_id("", None).is_err());
        assert!(resolve_application_id("   ", None).is_err());
    }

    #[test]
    fn sin_configurar_se_usa_el_id_incluido() {
        assert_eq!(
            resolve_application_id("", Some("1234567890123456789")).unwrap(),
            "1234567890123456789"
        );
        // Un config.toml de ejemplo trae `application_id = ""`: cuenta como vacío.
        assert_eq!(resolve_application_id("  ", Some("42")).unwrap(), "42");
    }

    #[test]
    fn el_config_del_usuario_gana_al_id_incluido() {
        assert_eq!(resolve_application_id("999", Some("42")).unwrap(), "999");
    }

    #[test]
    fn un_id_incluido_no_numerico_tambien_se_rechaza() {
        assert!(resolve_application_id("", Some("no-soy-un-id")).is_err());
    }

    #[test]
    fn rechaza_application_id_no_numerico() {
        let config = Config {
            application_id: "no-soy-un-id".into(),
            ..Config::default()
        };
        assert!(config.application_id().is_err());
    }

    #[test]
    fn acepta_application_id_valido() {
        let config = Config {
            application_id: "  1234567890123456789  ".into(),
            ..Config::default()
        };
        assert_eq!(config.application_id().unwrap(), "1234567890123456789");
    }

    #[test]
    fn toml_parcial_usa_defectos() {
        let config: Config = toml::from_str(r#"application_id = "123""#).unwrap();
        assert_eq!(config.application_id, "123");
        assert!(!config.privacy.share_server_address);
        assert!(config.privacy.share_save_name);
    }
}
