//! Instalación guiada: deja Steam configurado sin que el usuario toque rutas.
//!
//! La aplicación funciona de lanzador (`"…\factorio-discord-rp.exe" %command%`),
//! pero pegar esa línea a mano es engorroso. Este módulo la calcula con las rutas
//! reales y, si se le deja, la escribe en la configuración de Steam.
//!
//! La lógica vive aquí y no en el instalador para poder comprobarla con tests: el
//! asistente sólo llama a la aplicación y mira su código de salida.

mod clipboard;
mod options;
mod steam;
mod vdf;

use std::fmt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::paths;
use steam::{Steam, FACTORIO_APP_ID};

/// Cuánto se espera a que Steam termine de cerrarse.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Cerrar Steam si está abierto, en vez de negarse.
    pub close_steam: bool,
    /// Reabrir Steam después, sólo si lo hemos cerrado nosotros.
    pub restart_steam: bool,
}

/// Qué se quiere hacer con la configuración de Steam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Install,
    Uninstall,
}

/// Por qué ha fallado, con un código de salida distinto para que el instalador
/// decida qué le dice al usuario sin tener que interpretar textos.
#[derive(Debug)]
pub enum Failure {
    /// Steam está abierto y no se ha permitido cerrarlo. Código 10.
    SteamRunning,
    /// No hay Steam, o Factorio no está instalado en él. Código 11.
    NotFound(String),
    /// No se pudo leer o escribir la configuración. Código 12.
    Write(String),
    /// Cualquier otro error. Código 1.
    Other(String),
}

impl Failure {
    pub fn exit_code(&self) -> i32 {
        match self {
            Failure::Other(_) => 1,
            Failure::SteamRunning => 10,
            Failure::NotFound(_) => 11,
            Failure::Write(_) => 12,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::SteamRunning => write!(
                f,
                "Steam está abierto: hay que cerrarlo para cambiar su configuración"
            ),
            Failure::NotFound(msg) | Failure::Write(msg) | Failure::Other(msg) => {
                write!(f, "{msg}")
            }
        }
    }
}

impl From<anyhow::Error> for Failure {
    fn from(err: anyhow::Error) -> Self {
        Failure::Other(format!("{err:#}"))
    }
}

type Outcome = Result<(), Failure>;

fn exe() -> Result<String> {
    Ok(paths::current_exe()?.to_string_lossy().into_owned())
}

fn read_options(steam: &Steam) -> Result<Option<String>> {
    let path = steam.localconfig();
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("no se pudo leer {}", path.display()))?;
    vdf::launch_options(&text, FACTORIO_APP_ID)
}

/// La línea completa que hay que tener en las opciones de Steam.
///
/// Si se puede leer lo que ya hay, se calcula respetándolo; si no, es la línea
/// simple. Es lo que se le enseña al usuario que prefiere pegarla a mano.
fn resolved_line() -> Result<String> {
    let exe = exe()?;
    let existing = steam::locate()
        .and_then(|steam| read_options(&steam))
        .ok()
        .flatten()
        .unwrap_or_default();
    Ok(options::install(&existing, &exe))
}

/// Informe de lo detectado más la línea para pegar, copiada al portapapeles.
pub fn report() -> Outcome {
    let exe = exe()?;
    println!("Aplicación:        {exe}");

    match steam::locate() {
        Ok(steam) => {
            println!("Steam:             {}", steam.root.display());
            println!("Cuenta activa:     {}", steam.account);
            println!(
                "Factorio:          {}",
                if steam.factorio_installed() {
                    "instalado"
                } else {
                    "NO aparece instalado"
                }
            );
            println!(
                "Steam abierto:     {}",
                if steam::is_running() {
                    "sí (para aplicar el cambio hay que cerrarlo)"
                } else {
                    "no"
                }
            );
            match read_options(&steam) {
                Ok(Some(current)) => println!("Opciones actuales: {current}"),
                Ok(None) => println!("Opciones actuales: (ninguna)"),
                Err(err) => println!("Opciones actuales: no se pudieron leer ({err:#})"),
            }
        }
        Err(err) => println!("Steam:             no localizado ({err:#})"),
    }

    let line = resolved_line()?;
    println!();
    println!(
        "Línea completa para pegar en Steam → Factorio → Propiedades → Opciones de lanzamiento:"
    );
    println!();
    println!("{line}");
    println!();
    match clipboard::copy(&line) {
        Ok(()) => println!("(Copiada al portapapeles.)"),
        Err(err) => println!("(No se pudo copiar al portapapeles: {err:#})"),
    }
    Ok(())
}

/// Imprime únicamente la línea, sin nada más: la consume el instalador.
pub fn print_command() -> Outcome {
    println!("{}", resolved_line()?);
    Ok(())
}

/// Copia la línea al portapapeles y la imprime.
pub fn copy_command() -> Outcome {
    let line = resolved_line()?;
    clipboard::copy(&line)?;
    println!("{line}");
    Ok(())
}

/// Activa o desactiva el arranque con Windows.
pub fn set_autostart(enabled: bool) -> Outcome {
    crate::autostart::set_enabled(enabled)?;
    Ok(())
}

/// Lo que cambiaría en la configuración de Steam.
struct Plan {
    before: Option<String>,
    after: Option<String>,
    /// Contenido completo del fichero ya modificado.
    text: String,
}

impl Plan {
    fn changes_anything(&self) -> bool {
        self.before != self.after
    }
}

fn plan(text: &str, exe: &str, action: Action) -> Result<Plan> {
    let before = vdf::launch_options(text, FACTORIO_APP_ID)?;
    let current = before.clone().unwrap_or_default();

    let target = match action {
        Action::Install => options::install(&current, exe),
        Action::Uninstall => options::uninstall(&current, exe),
    };
    // Vacío = no queda nada que guardar: se borra la clave en vez de dejarla en blanco.
    let after = (!target.is_empty()).then_some(target);

    let text = if after == before {
        text.to_string()
    } else {
        vdf::set_launch_options(text, FACTORIO_APP_ID, after.as_deref())?
    };

    Ok(Plan {
        before,
        after,
        text,
    })
}

fn locate_steam() -> Result<Steam, Failure> {
    steam::locate().map_err(|err| Failure::NotFound(format!("{err:#}")))
}

fn read_config(steam: &Steam) -> Result<String, Failure> {
    let path = steam.localconfig();
    std::fs::read_to_string(&path)
        .map_err(|err| Failure::Write(format!("no se pudo leer {}: {err}", path.display())))
}

/// Muestra qué cambiaría, sin escribir nada.
pub fn dry_run(action: Action) -> Outcome {
    let exe = exe()?;
    let steam = locate_steam()?;
    let text = read_config(&steam)?;
    let plan = plan(&text, &exe, action)?;

    println!("Fichero:  {}", steam.localconfig().display());
    println!(
        "Steam:    {}",
        if steam::is_running() {
            "abierto (para aplicarlo de verdad habría que cerrarlo)"
        } else {
            "cerrado"
        }
    );
    println!(
        "Antes:    {}",
        plan.before.as_deref().unwrap_or("(sin opciones)")
    );
    println!(
        "Después:  {}",
        plan.after.as_deref().unwrap_or("(sin opciones)")
    );
    if plan.changes_anything() {
        println!("Se haría copia de seguridad del fichero y se cambiaría sólo esa clave.");
    } else {
        println!("No habría ningún cambio.");
    }
    println!("No se ha escrito nada (--dry-run).");
    Ok(())
}

/// Pone la aplicación en las opciones de lanzamiento de Factorio.
pub fn apply(opts: &Options) -> Outcome {
    change(opts, Action::Install)
}

/// Quita la aplicación de las opciones de lanzamiento, dejando las demás.
pub fn uninstall(opts: &Options) -> Outcome {
    change(opts, Action::Uninstall)
}

fn change(opts: &Options, action: Action) -> Outcome {
    let exe = exe()?;
    let steam = locate_steam()?;

    if action == Action::Install && !steam.factorio_installed() {
        return Err(Failure::NotFound(
            "Factorio no aparece instalado en ninguna biblioteca de Steam".into(),
        ));
    }

    // Steam reescribe localconfig.vdf mientras corre: editarlo abierto se pierde.
    let mut closed_by_us = false;
    if steam::is_running() {
        if !opts.close_steam {
            return Err(Failure::SteamRunning);
        }
        println!("Cerrando Steam…");
        steam
            .shutdown(SHUTDOWN_TIMEOUT)
            .map_err(|err| Failure::Other(format!("{err:#}")))?;
        closed_by_us = true;
    }

    let result = rewrite(&steam, &exe, action);

    // Se reabre aunque el cambio haya fallado: no hay que dejar a nadie sin Steam.
    if closed_by_us && opts.restart_steam {
        println!("Reabriendo Steam…");
        if let Err(err) = steam.start() {
            eprintln!("aviso: no se pudo reabrir Steam: {err:#}");
        }
    }

    result
}

fn rewrite(steam: &Steam, exe: &str, action: Action) -> Outcome {
    let path = steam.localconfig();
    let text = read_config(steam)?;
    let plan = plan(&text, exe, action)?;

    if !plan.changes_anything() {
        println!(
            "{}",
            match action {
                Action::Install => "Ya estaba configurado: no hay nada que cambiar.",
                Action::Uninstall => "No había nada que quitar.",
            }
        );
        return Ok(());
    }

    let backup = backup_path(&path);
    std::fs::copy(&path, &backup).map_err(|err| {
        Failure::Write(format!(
            "no se pudo hacer la copia de seguridad {}: {err}",
            backup.display()
        ))
    })?;
    println!("Copia de seguridad: {}", backup.display());

    write_atomically(&path, &plan.text)
        .map_err(|err| Failure::Write(format!("no se pudo escribir {}: {err}", path.display())))?;

    println!(
        "Opciones de lanzamiento de Factorio: {}",
        plan.after.as_deref().unwrap_or("(sin opciones)")
    );
    Ok(())
}

/// Escribe en un fichero temporal y lo renombra encima: si algo falla a medias,
/// el original queda intacto en vez de truncado.
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let temp = path.with_extension("vdf.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}

fn backup_path(path: &Path) -> std::path::PathBuf {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    path.with_extension(format!("vdf.bak-{}", utc_stamp(secs)))
}

/// `AAAAMMDD-HHMMSS` en UTC, sin depender de una biblioteca de fechas.
fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);

    // Días desde 1970 → fecha civil (algoritmo de Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);

    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str =
        r"C:\Users\villa\AppData\Local\Programs\Factorio Discord RP\factorio-discord-rp.exe";

    /// Fichero mínimo con la estructura real: `apps` → Factorio.
    fn sample() -> String {
        format!(
            "\"UserLocalConfigStore\"\n{{\n\t\"apps\"\n\t{{\n\t\t\"{FACTORIO_APP_ID}\"\n\t\t{{\n\t\t\t\"playtime\"\t\t\"54904\"\n\t\t}}\n\t}}\n}}\n"
        )
    }

    #[test]
    fn instalar_y_desinstalar_dejan_el_fichero_como_estaba() {
        let original = sample();

        let instalado = plan(&original, EXE, Action::Install).unwrap();
        assert!(instalado.changes_anything());
        assert_eq!(
            instalado.after.as_deref(),
            Some(format!("\"{EXE}\" %command%").as_str())
        );

        let quitado = plan(&instalado.text, EXE, Action::Uninstall).unwrap();
        assert_eq!(quitado.after, None);
        assert_eq!(quitado.text, original);
    }

    #[test]
    fn instalar_dos_veces_no_cambia_nada_la_segunda() {
        let una = plan(&sample(), EXE, Action::Install).unwrap();
        let dos = plan(&una.text, EXE, Action::Install).unwrap();
        assert!(!dos.changes_anything());
        assert_eq!(dos.text, una.text);
    }

    #[test]
    fn desinstalar_sin_estar_instalada_no_cambia_nada() {
        let plan = plan(&sample(), EXE, Action::Uninstall).unwrap();
        assert!(!plan.changes_anything());
        assert_eq!(plan.text, sample());
    }

    #[test]
    fn respeta_las_opciones_que_ya_tenia_el_usuario() {
        let con_opciones =
            vdf::set_launch_options(&sample(), FACTORIO_APP_ID, Some("%command% -x")).unwrap();

        let instalado = plan(&con_opciones, EXE, Action::Install).unwrap();
        assert_eq!(
            instalado.after.as_deref(),
            Some(format!("\"{EXE}\" %command% -x").as_str())
        );

        let quitado = plan(&instalado.text, EXE, Action::Uninstall).unwrap();
        assert_eq!(quitado.after.as_deref(), Some("%command% -x"));
    }

    #[test]
    fn los_codigos_de_salida_son_los_documentados() {
        assert_eq!(Failure::SteamRunning.exit_code(), 10);
        assert_eq!(Failure::NotFound(String::new()).exit_code(), 11);
        assert_eq!(Failure::Write(String::new()).exit_code(), 12);
        assert_eq!(Failure::Other(String::new()).exit_code(), 1);
    }

    #[test]
    fn el_sello_de_la_copia_es_una_fecha_legible() {
        assert_eq!(utc_stamp(0), "19700101-000000");
        assert_eq!(utc_stamp(1_000_000_000), "20010909-014640");
        // 29 de febrero de un año bisiesto.
        assert_eq!(utc_stamp(951_782_400), "20000229-000000");
    }

    #[test]
    fn la_copia_lleva_el_sello_en_el_nombre() {
        let copia = backup_path(Path::new(r"C:\Steam\userdata\1\config\localconfig.vdf"));
        let nombre = copia.file_name().unwrap().to_string_lossy().into_owned();
        assert!(nombre.starts_with("localconfig.vdf.bak-"), "{nombre}");
    }
}
