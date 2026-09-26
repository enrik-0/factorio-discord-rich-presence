//! Composición de la línea de lanzamiento de Steam alrededor de `%command%`.
//!
//! Steam sustituye `%command%` por el ejecutable del juego con sus argumentos.
//! Para que la aplicación actúe de lanzador basta con ponerla justo delante:
//! `"C:\ruta\factorio-discord-rp.exe" %command%`. Si el usuario ya tenía opciones,
//! se respetan y la aplicación se cuela delante de `%command%`, sin perderlas.

const COMMAND: &str = "%command%";

/// La ruta del ejecutable entre comillas, tal y como aparece en la línea.
fn quoted(exe: &str) -> String {
    format!("\"{exe}\"")
}

/// Posición de `needle` en `haystack` sin distinguir mayúsculas de ASCII.
///
/// Las rutas de Windows no distinguen mayúsculas. `to_ascii_lowercase` conserva
/// la longitud en bytes, así que la posición sirve para cortar el original.
fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}

/// Nombre del ejecutable, con el que se reconoce un lanzador nuestro de otra ruta.
const APP_FILE: &str = "factorio-discord-rp.exe";

/// Rango de un lanzador nuestro que ya esté en las opciones, sea cual sea su ruta:
/// desde la comilla que abre su ruta hasta justo antes de `%command%`. Incluye los
/// argumentos que llevara (`--config …`), que son suyos y no del juego.
///
/// Es el caso de quien pegó la línea a mano y luego usa el instalador: hay que
/// sustituirlo, no dejar dos lanzadores uno dentro de otro.
fn find_previous_launcher(existing: &str) -> Option<(usize, usize)> {
    let lower = existing.to_ascii_lowercase();
    let file = lower.find(&format!("{APP_FILE}\""))?;
    let start = lower[..file].rfind('"')?;
    let command = lower.find(COMMAND)?;
    (command > start).then_some((start, command))
}

/// La línea completa para el caso más simple, sin opciones previas.
pub fn command_line(exe: &str) -> String {
    format!("{} {COMMAND}", quoted(exe))
}

/// ¿Ya está la aplicación en las opciones?
pub fn is_installed(existing: &str, exe: &str) -> bool {
    find_ignore_case(existing, &quoted(exe)).is_some()
}

/// Opciones resultantes de añadir la aplicación a las que hubiera.
///
/// - sin opciones → `"exe" %command%`
/// - con `%command%` → se antepone a la primera aparición
/// - opciones sueltas (sin `%command%`) → `"exe" %command% <opciones>`, que
///   conserva su efecto: Steam las añadiría detrás del juego igualmente
/// - ya instalada → sin cambios
pub fn install(existing: &str, exe: &str) -> String {
    let existing = existing.trim();
    if is_installed(existing, exe) {
        return existing.to_string();
    }

    let ours = quoted(exe);
    if existing.is_empty() {
        return command_line(exe);
    }

    // Un lanzador nuestro de otra ruta (p. ej. pegado a mano antes) se sustituye.
    if let Some((start, command)) = find_previous_launcher(existing) {
        return format!("{}{ours} {}", &existing[..start], &existing[command..]);
    }

    match existing.find(COMMAND) {
        Some(pos) => format!("{}{ours} {}", &existing[..pos], &existing[pos..]),
        None => format!("{ours} {COMMAND} {existing}"),
    }
}

/// Opciones resultantes de quitar la aplicación. Una cadena vacía significa
/// que no queda nada que valga la pena guardar: hay que borrar la clave.
///
/// Si lo único que sobrevive es `%command%` se considera vacío: equivale a no
/// tener opciones y deja el fichero como estaba antes de instalar.
pub fn uninstall(existing: &str, exe: &str) -> String {
    let existing = existing.trim();
    let ours = quoted(exe);

    // Se quita nuestro lanzador con sus argumentos, esté donde esté el ejecutable;
    // si no hay `%command%` que lo delimite, sólo la ruta exacta de esta instalación.
    let result = if let Some((start, command)) = find_previous_launcher(existing) {
        format!("{}{}", &existing[..start], &existing[command..])
            .trim()
            .to_string()
    } else if let Some(pos) = find_ignore_case(existing, &ours) {
        let after = existing[pos + ours.len()..].trim_start();
        format!("{}{after}", &existing[..pos]).trim().to_string()
    } else {
        return existing.to_string();
    };

    if result == COMMAND {
        String::new()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str =
        r"C:\Users\villa\AppData\Local\Programs\Factorio Discord RP\factorio-discord-rp.exe";

    fn nuestra() -> String {
        format!("\"{EXE}\"")
    }

    #[test]
    fn sin_opciones_previas_queda_la_linea_simple() {
        assert_eq!(install("", EXE), format!("{} %command%", nuestra()));
        assert_eq!(install("   ", EXE), command_line(EXE));
    }

    #[test]
    fn la_linea_completa_lleva_la_ruta_entera_entre_comillas() {
        // Es lo que se le enseña al usuario para pegar: debe ser autosuficiente.
        let linea = command_line(EXE);
        assert!(linea.starts_with('"') && linea.contains(EXE));
        assert!(linea.ends_with("%command%"));
    }

    #[test]
    fn con_command_se_antepone_a_la_primera_aparicion() {
        assert_eq!(
            install("%command% -foo", EXE),
            format!("{} %command% -foo", nuestra())
        );
        assert_eq!(
            install("otro %command%", EXE),
            format!("otro {} %command%", nuestra())
        );
    }

    #[test]
    fn opciones_sueltas_se_conservan_detras_del_juego() {
        assert_eq!(
            install("--mod-directory X", EXE),
            format!("{} %command% --mod-directory X", nuestra())
        );
    }

    #[test]
    fn instalar_dos_veces_no_duplica() {
        let una = install("%command% -x", EXE);
        assert_eq!(install(&una, EXE), una);
    }

    #[test]
    fn detecta_la_instalacion_sin_distinguir_mayusculas() {
        assert!(is_installed(&command_line(EXE), &EXE.to_uppercase()));
        assert!(!is_installed("%command%", EXE));
    }

    #[test]
    fn desinstalar_devuelve_lo_que_habia() {
        for antes in [
            "",
            "%command% -x",
            "gamemoderun %command%",
            "otro %command% -y",
        ] {
            let despues = uninstall(&install(antes, EXE), EXE);
            let esperado = if antes == "%command%" { "" } else { antes };
            assert_eq!(despues, esperado, "ida y vuelta de {antes:?}");
        }
    }

    #[test]
    fn desinstalar_lo_unico_que_habia_deja_vacio() {
        assert_eq!(uninstall(&command_line(EXE), EXE), "");
    }

    #[test]
    fn opciones_sueltas_quedan_como_command_mas_opciones() {
        // No es idéntico al original, pero sí equivalente para Steam.
        assert_eq!(
            uninstall(&install("--mod-directory X", EXE), EXE),
            "%command% --mod-directory X"
        );
    }

    #[test]
    fn desinstalar_sin_estar_instalada_no_toca_nada() {
        assert_eq!(uninstall("%command% -x", EXE), "%command% -x");
    }

    /// La línea que tenía de verdad un usuario que la pegó a mano (con `--config`).
    const PEGADA_A_MANO: &str = r#""D:\proyectos\factorio discord rich presence\target\release\factorio-discord-rp.exe" --config "D:\proyectos\factorio discord rich presence\config.toml" %command%"#;

    #[test]
    fn un_lanzador_pegado_a_mano_se_sustituye_no_se_anida() {
        assert_eq!(
            install(PEGADA_A_MANO, EXE),
            format!("{} %command%", nuestra())
        );
    }

    #[test]
    fn el_lanzador_anterior_se_sustituye_conservando_lo_del_usuario() {
        let antes = r#""C:\dev\factorio-discord-rp.exe" --config x %command% -foo"#;
        assert_eq!(install(antes, EXE), format!("{} %command% -foo", nuestra()));
    }

    #[test]
    fn desinstalar_quita_el_lanzador_de_cualquier_ruta() {
        assert_eq!(uninstall(PEGADA_A_MANO, EXE), "");
        assert_eq!(
            uninstall(
                r#"otro "C:\dev\factorio-discord-rp.exe" --config x %command% -y"#,
                EXE
            ),
            "otro %command% -y"
        );
    }

    #[test]
    fn una_ruta_con_espacios_y_acentos_se_trata_igual() {
        let exe = r"C:\Users\Ñandú Pérez\Mis Juegos\drp.exe";
        assert_eq!(uninstall(&install("%command%", exe), exe), "");
    }
}
