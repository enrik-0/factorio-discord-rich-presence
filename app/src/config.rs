//! Loading of `config.toml`.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tracing::debug;

/// Lets tests run without touching the config file.
const ENV_APPLICATION_ID: &str = "FACTORIO_DRP_APP_ID";

/// Discord Application ID baked in at compile time, so whoever installs the
/// app doesn't have to create their own. Injected by CI when building the
/// installer (`FACTORIO_DRP_DEFAULT_APP_ID`); in a normal build it doesn't
/// exist and the ID comes from `config.toml`. It's public data, but this way
/// it doesn't live in the source code.
const INCLUDED_APPLICATION_ID: Option<&str> = option_env!("FACTORIO_DRP_DEFAULT_APP_ID");

/// Picks the ID to use and checks that it's valid.
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
            "missing Discord Application ID.\n\
             Create one at https://discord.com/developers/applications and set it in \
             config.toml (application_id field) or in the {ENV_APPLICATION_ID} environment variable."
        );
    }
    if !id.chars().all(|c| c.is_ascii_digit()) {
        bail!("the Application ID must be digits only, received: {id:?}");
    }
    Ok(id)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Discord Application ID. Mandatory.
    pub application_id: String,
    /// Factorio data folder. Empty = auto-detect.
    pub factorio_data_dir: String,
    /// Asset key uploaded to the Discord developer portal. Empty = no image.
    pub large_image: String,
    pub privacy: Privacy,
    /// Advanced mode. Absent = automatic layout based on the mod's settings.
    pub templates: Option<Templates>,
}

/// How the data is laid out across the few slots the Discord card offers.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Templates {
    /// First line.
    pub details: String,
    /// Second line.
    pub state: String,
    /// Icon tooltip.
    pub large_text: String,
    pub fallback: Fallback,
}

/// Fallback templates, for when the main one runs out of data.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Fallback {
    /// Used when there's no research queued.
    pub state: String,
}

impl Default for Templates {
    fn default() -> Self {
        Self {
            details: "{planet} · {save}".into(),
            state: "Researching {research} ({research_pct}%)".into(),
            // Without a small icon, the game mode takes refuge in this tooltip.
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
    /// Never on by default: it exposes the server's IP on the public profile.
    pub share_server_address: bool,
    pub share_save_name: bool,
}

// Neither `Config` nor `Privacy` can derive Default: they start out with values.
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
    /// Loads the configuration, searching in order:
    ///   1. the explicit path, if `--config` was passed
    ///   2. `config.toml` next to the executable
    ///   3. `%APPDATA%/factorio-discord-rp/config.toml`
    ///   4. `config.toml` in the current directory
    ///
    /// If none of these are found, the defaults are used, so the environment
    /// variable alone is enough for a quick test.
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let path = match explicit {
            Some(path) => {
                if !path.exists() {
                    bail!("config file does not exist: {}", path.display());
                }
                Some(path.to_path_buf())
            }
            None => Self::discover(),
        };

        let mut config = match &path {
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .with_context(|| format!("could not read {}", path.display()))?;
                let config: Config = toml::from_str(&text)
                    .with_context(|| format!("{} is not valid TOML", path.display()))?;
                debug!(path = %path.display(), "configuration loaded");
                config
            }
            None => {
                debug!("no config.toml found; using defaults");
                Config::default()
            }
        };

        if let Ok(id) = std::env::var(ENV_APPLICATION_ID) {
            if !id.trim().is_empty() {
                debug!("application_id taken from {ENV_APPLICATION_ID}");
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
        // Stable location: with autostart, the current directory is System32,
        // so it can't be relied on.
        if let Some(path) = crate::paths::config_in_app_dir() {
            candidates.push(path);
        }
        candidates.push(PathBuf::from(crate::paths::CONFIG_FILE));
        candidates.into_iter().find(|path| path.exists())
    }

    /// Validates the bare minimum needed to talk to Discord.
    ///
    /// Sends whatever is in `config.toml` (or the environment variable); if
    /// it's empty, the ID baked into the installer at compile time is used.
    pub fn application_id(&self) -> Result<&str> {
        resolve_application_id(&self.application_id, INCLUDED_APPLICATION_ID)
    }

    /// Factorio data folder: the one containing `script-output`, `saves`,
    /// and `factorio-current.log`.
    pub fn factorio_data_dir(&self) -> Result<PathBuf> {
        if !self.factorio_data_dir.trim().is_empty() {
            let path = PathBuf::from(self.factorio_data_dir.trim());
            if !path.is_dir() {
                bail!("factorio_data_dir is not a directory: {}", path.display());
            }
            return Ok(path);
        }

        let path = default_factorio_data_dir()?;
        if !path.is_dir() {
            bail!(
                "could not find the Factorio data folder at {}. \
                 Set it manually in config.toml (factorio_data_dir).",
                path.display()
            );
        }
        Ok(path)
    }
}

/// Default location of Factorio's data folder: `%APPDATA%\Factorio` on
/// Windows, `~/.factorio` on Unix (where Factorio actually puts it on Linux).
#[cfg(windows)]
fn default_factorio_data_dir() -> Result<PathBuf> {
    let appdata =
        std::env::var("APPDATA").context("could not read %APPDATA% to locate Factorio")?;
    Ok(PathBuf::from(appdata).join("Factorio"))
}

#[cfg(unix)]
fn default_factorio_data_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("could not read $HOME to locate Factorio")?;
    Ok(PathBuf::from(home).join(".factorio"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_empty_application_id() {
        // No value baked in at compile time: so it doesn't depend on how it was built.
        assert!(resolve_application_id("", None).is_err());
        assert!(resolve_application_id("   ", None).is_err());
    }

    #[test]
    fn falls_back_to_the_included_id_when_unconfigured() {
        assert_eq!(
            resolve_application_id("", Some("1234567890123456789")).unwrap(),
            "1234567890123456789"
        );
        // A sample config.toml ships with `application_id = ""`: counts as empty.
        assert_eq!(resolve_application_id("  ", Some("42")).unwrap(), "42");
    }

    #[test]
    fn the_users_config_wins_over_the_included_id() {
        assert_eq!(resolve_application_id("999", Some("42")).unwrap(), "999");
    }

    #[test]
    fn a_non_numeric_included_id_is_also_rejected() {
        assert!(resolve_application_id("", Some("not-an-id")).is_err());
    }

    #[test]
    fn rejects_a_non_numeric_application_id() {
        let config = Config {
            application_id: "not-an-id".into(),
            ..Config::default()
        };
        assert!(config.application_id().is_err());
    }

    #[test]
    fn accepts_a_valid_application_id() {
        let config = Config {
            application_id: "  1234567890123456789  ".into(),
            ..Config::default()
        };
        assert_eq!(config.application_id().unwrap(), "1234567890123456789");
    }

    #[test]
    fn partial_toml_falls_back_to_defaults() {
        let config: Config = toml::from_str(r#"application_id = "123""#).unwrap();
        assert_eq!(config.application_id, "123");
        assert!(!config.privacy.share_server_address);
        assert!(config.privacy.share_save_name);
    }
}
