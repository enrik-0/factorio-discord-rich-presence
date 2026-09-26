//! Edición mínima de `LaunchOptions` en el `localconfig.vdf` de Steam.
//!
//! No es un analizador VDF completo. Sólo hace falta localizar el bloque de una
//! aplicación (`apps` → `<id>`) y leer, cambiar o quitar una clave suya, dejando
//! intacto el resto del fichero byte a byte: sangría, saltos de línea y todo lo
//! que Steam haya escrito. Un fichero de 650 KB con cientos de bloques no se
//! reserializa; se corta y se cose por la posición exacta.

use anyhow::{bail, Result};

const KEY: &str = "LaunchOptions";

/// Un símbolo del texto con sus posiciones en bytes.
///
/// Se trabaja sobre bytes porque `"`, `\`, `{` y `}` son ASCII y nunca aparecen
/// dentro de una secuencia UTF-8 multibyte: cortar en ellos es siempre válido.
#[derive(Debug, Clone, Copy)]
enum Token {
    /// Cadena entrecomillada: `start` es el primer byte tras la comilla de
    /// apertura y `end` el de la comilla de cierre (contenido = `start..end`).
    Str {
        start: usize,
        end: usize,
    },
    Open,
    Close(usize),
}

fn tokenize(text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j] != b'"' {
                    if bytes[j] == b'\\' {
                        j += 1; // el carácter escapado no cierra la cadena
                    }
                    j += 1;
                }
                let end = j.min(bytes.len());
                tokens.push(Token::Str { start, end });
                i = end + 1;
            }
            b'{' => {
                tokens.push(Token::Open);
                i += 1;
            }
            b'}' => {
                tokens.push(Token::Close(i));
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }

    tokens
}

/// Deshace el escapado VDF: `\\` → `\`, `\"` → `"`, `\n`, `\t`.
pub fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Escapa un valor para escribirlo entre comillas: `\` → `\\`, `"` → `\"`.
pub fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Posición de un bloque en la lista de símbolos: su `{` y su `}`.
struct Block {
    open: usize,
    close: usize,
}

fn matching_close(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::Open => depth += 1,
            Token::Close(_) => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            Token::Str { .. } => {}
        }
    }
    None
}

/// Localiza `apps` → `<app_id> { … }`.
///
/// Se exige que el padre sea `apps`: el mismo número aparece en otras secciones
/// de `localconfig.vdf` como valor suelto, y no son el bloque de la aplicación.
fn find_app_block(text: &str, tokens: &[Token], app_id: &str) -> Option<Block> {
    let mut stack: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < tokens.len() {
        match tokens[i] {
            Token::Str { start, end } if matches!(tokens.get(i + 1), Some(Token::Open)) => {
                let key = &text[start..end];
                let parent_is_apps = stack.last().is_some_and(|p| p.eq_ignore_ascii_case("apps"));
                if key == app_id && parent_is_apps {
                    let open = i + 1;
                    let close = matching_close(tokens, open)?;
                    return Some(Block { open, close });
                }
                stack.push(key);
                i += 2; // clave y llave
                continue;
            }
            Token::Close(_) => {
                stack.pop();
            }
            _ => {}
        }
        i += 1;
    }

    None
}

/// Dónde está `LaunchOptions` dentro de un bloque: comillas de apertura de la
/// clave y rango del contenido del valor.
struct Found {
    key_quote: usize,
    value_start: usize,
    value_end: usize,
}

/// Busca la clave sólo entre los hijos directos del bloque, sin bajar a los
/// sub-bloques (`cloud`, `autocloud`…), que podrían tener claves homónimas.
fn find_launch_options(text: &str, tokens: &[Token], block: &Block) -> Option<Found> {
    let mut depth = 0i32;
    let mut i = block.open + 1;

    while i < block.close {
        match tokens[i] {
            Token::Str { start, end } if depth == 0 => match tokens.get(i + 1) {
                Some(Token::Str {
                    start: value_start,
                    end: value_end,
                }) => {
                    if text[start..end] == *KEY {
                        return Some(Found {
                            key_quote: start - 1,
                            value_start: *value_start,
                            value_end: *value_end,
                        });
                    }
                    i += 2;
                    continue;
                }
                Some(Token::Open) => {
                    depth += 1;
                    i += 2;
                    continue;
                }
                _ => {}
            },
            Token::Close(_) => depth -= 1,
            _ => {}
        }
        i += 1;
    }

    None
}

fn newline_style(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Inicio de la línea que contiene la posición `pos`.
fn line_start(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

fn app_block(text: &str, tokens: &[Token], app_id: &str) -> Result<Block> {
    match find_app_block(text, tokens, app_id) {
        Some(block) => Ok(block),
        None => bail!(
            "no hay bloque de la aplicación {app_id} en localconfig.vdf; \
             Steam lo crea la primera vez que se abre el juego desde su biblioteca"
        ),
    }
}

/// Opciones de lanzamiento actuales, o `None` si no hay ninguna definida.
pub fn launch_options(text: &str, app_id: &str) -> Result<Option<String>> {
    let tokens = tokenize(text);
    let block = app_block(text, &tokens, app_id)?;
    Ok(find_launch_options(text, &tokens, &block)
        .map(|found| unescape(&text[found.value_start..found.value_end])))
}

/// Devuelve el texto con `LaunchOptions` cambiado.
///
/// - `Some(valor)`: sustituye el valor, o añade la clave al final del bloque.
/// - `None`: quita la clave. Si no existía, el texto queda igual.
pub fn set_launch_options(text: &str, app_id: &str, value: Option<&str>) -> Result<String> {
    let tokens = tokenize(text);
    let block = app_block(text, &tokens, app_id)?;
    let found = find_launch_options(text, &tokens, &block);

    match (found, value) {
        (Some(found), Some(value)) => Ok(format!(
            "{}{}{}",
            &text[..found.value_start],
            escape(value),
            &text[found.value_end..]
        )),
        (Some(found), None) => Ok(remove_pair(text, &found)),
        (None, Some(value)) => Ok(insert_pair(text, &tokens, &block, value)),
        (None, None) => Ok(text.to_string()),
    }
}

/// Quita el par clave-valor, y su línea entera si no hay nada más en ella.
fn remove_pair(text: &str, found: &Found) -> String {
    let pair_start = found.key_quote;
    let pair_end = found.value_end + 1; // tras la comilla de cierre del valor

    let start_of_line = line_start(text, pair_start);
    let only_indent_before = text[start_of_line..pair_start]
        .chars()
        .all(|c| c == ' ' || c == '\t');

    let after = &text[pair_end..];
    let rest_of_line = after.find('\n').unwrap_or(after.len());
    let only_blank_after = after[..rest_of_line]
        .chars()
        .all(|c| c == ' ' || c == '\t' || c == '\r');

    if only_indent_before && only_blank_after {
        let end = (pair_end + rest_of_line + 1).min(text.len());
        format!("{}{}", &text[..start_of_line], &text[end..])
    } else {
        format!("{}{}", &text[..pair_start], &text[pair_end..])
    }
}

/// Añade `"LaunchOptions"  "valor"` justo antes de la llave que cierra el bloque,
/// con la sangría de sus hermanos y el estilo de saltos de línea del fichero.
fn insert_pair(text: &str, tokens: &[Token], block: &Block, value: &str) -> String {
    let close_pos = match tokens[block.close] {
        Token::Close(pos) => pos,
        _ => unreachable!("el cierre de un bloque es siempre una llave"),
    };
    let nl = newline_style(text);
    let pair = format!("\"{KEY}\"\t\t\"{}\"", escape(value));

    let start_of_line = line_start(text, close_pos);
    let indent_of_close = &text[start_of_line..close_pos];

    if indent_of_close.chars().all(|c| c == ' ' || c == '\t') {
        // La `}` está sola en su línea: el par entra una sangría más adentro.
        let line = format!("{indent_of_close}\t{pair}{nl}");
        format!(
            "{}{}{}",
            &text[..start_of_line],
            line,
            &text[start_of_line..]
        )
    } else {
        // `{ ... }` en una sola línea: se inserta pegado a la llave.
        format!("{} {pair} {}", &text[..close_pos], &text[close_pos..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Estructura y sangría copiadas de un `localconfig.vdf` real, más un valor
    /// suelto `"427520"` en otra sección, que no debe confundirse con el bloque.
    const SAMPLE: &str = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"427520\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1790455585\"\n\t\t\t\t\t\t\"cloud\"\n\t\t\t\t\t\t{\n\t\t\t\t\t\t\t\"last_sync_state\"\t\t\"synchronized\"\n\t\t\t\t\t\t}\n\t\t\t\t\t\t\"playtime\"\t\t\"54904\"\n\t\t\t\t\t}\n\t\t\t\t\t\"999\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"-otra-app\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n\t\"Otra\"\n\t{\n\t\t\"427520\"\t\t\"3800000004000000968771100100100100860600faee\"\n\t}\n}\n";

    fn con_opciones(valor: &str) -> String {
        SAMPLE.replace(
            "\t\t\t\t\t\t\"playtime\"\t\t\"54904\"\n",
            &format!("\t\t\t\t\t\t\"playtime\"\t\t\"54904\"\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"{valor}\"\n"),
        )
    }

    #[test]
    fn sin_la_clave_no_hay_opciones() {
        assert_eq!(launch_options(SAMPLE, "427520").unwrap(), None);
    }

    #[test]
    fn el_valor_suelto_de_otra_seccion_no_es_el_bloque() {
        // Si se confundiera, `launch_options` daría error o leería otra cosa.
        assert!(launch_options(SAMPLE, "427520").is_ok());
        assert_eq!(
            launch_options(SAMPLE, "999").unwrap().as_deref(),
            Some("-otra-app")
        );
    }

    #[test]
    fn una_aplicacion_ausente_es_un_error_claro() {
        let err = launch_options(SAMPLE, "12345").unwrap_err().to_string();
        assert!(err.contains("12345"), "{err}");
    }

    #[test]
    fn anadir_la_clave_respeta_la_sangria_y_el_resto() {
        let out =
            set_launch_options(SAMPLE, "427520", Some("\"C:\\a b\\x.exe\" %command%")).unwrap();

        assert_eq!(
            launch_options(&out, "427520").unwrap().as_deref(),
            Some("\"C:\\a b\\x.exe\" %command%")
        );
        // Sangría de sus hermanos (6 tabuladores) y escapado en el fichero.
        assert!(out.contains(
            "\t\t\t\t\t\t\"LaunchOptions\"\t\t\"\\\"C:\\\\a b\\\\x.exe\\\" %command%\"\n\t\t\t\t\t}\n\t\t\t\t\t\"999\""
        ));
        // Todo lo demás, intacto: quitando la línea nueva se recupera el original.
        assert_eq!(set_launch_options(&out, "427520", None).unwrap(), SAMPLE);
    }

    #[test]
    fn no_toca_las_opciones_de_otras_aplicaciones() {
        let out = set_launch_options(SAMPLE, "427520", Some("%command%")).unwrap();
        assert_eq!(
            launch_options(&out, "999").unwrap().as_deref(),
            Some("-otra-app")
        );
    }

    #[test]
    fn sustituir_un_valor_existente() {
        let antes = con_opciones("-viejo");
        let out = set_launch_options(&antes, "427520", Some("%command% -nuevo")).unwrap();
        assert_eq!(
            launch_options(&out, "427520").unwrap().as_deref(),
            Some("%command% -nuevo")
        );
        assert_eq!(out.matches("LaunchOptions").count(), 2, "una por app");
    }

    #[test]
    fn quitar_la_clave_borra_su_linea_entera() {
        let antes = con_opciones("-viejo");
        let out = set_launch_options(&antes, "427520", None).unwrap();
        assert_eq!(out, SAMPLE);
    }

    #[test]
    fn quitar_una_clave_inexistente_no_cambia_nada() {
        assert_eq!(set_launch_options(SAMPLE, "427520", None).unwrap(), SAMPLE);
    }

    #[test]
    fn el_escapado_es_reversible() {
        let valor = "\"C:\\Program Files\\x.exe\" --a=\"b\" %command%";
        assert_eq!(unescape(&escape(valor)), valor);
    }

    #[test]
    fn respeta_los_saltos_de_linea_de_windows() {
        let crlf = SAMPLE.replace('\n', "\r\n");
        let out = set_launch_options(&crlf, "427520", Some("%command%")).unwrap();
        assert!(!out.replace("\r\n", "").contains('\n'), "sin \\n sueltos");
        assert!(out.contains("\"LaunchOptions\"\t\t\"%command%\"\r\n"));
        assert_eq!(set_launch_options(&out, "427520", None).unwrap(), crlf);
    }

    #[test]
    fn ignora_una_clave_homonima_en_un_subbloque() {
        let con_subbloque = SAMPLE.replace(
            "\"last_sync_state\"\t\t\"synchronized\"",
            "\"LaunchOptions\"\t\t\"no-soy-yo\"",
        );
        assert_eq!(launch_options(&con_subbloque, "427520").unwrap(), None);
    }

    #[test]
    fn un_valor_con_llaves_o_barras_no_rompe_el_analisis() {
        let raro = con_opciones("--x={y} // z");
        assert_eq!(
            launch_options(&raro, "427520").unwrap().as_deref(),
            Some("--x={y} // z")
        );
        assert_eq!(
            launch_options(&raro, "999").unwrap().as_deref(),
            Some("-otra-app")
        );
    }

    #[test]
    fn el_texto_con_acentos_se_conserva() {
        let con_acentos = SAMPLE.replace("\"Otra\"", "\"Ñandú ó\"");
        let out = set_launch_options(&con_acentos, "427520", Some("%command%")).unwrap();
        assert!(out.contains("\"Ñandú ó\""));
    }

    /// Comprobación contra un fichero de verdad, que no cabe en el repositorio:
    /// `STEAM_LOCALCONFIG=<ruta> cargo test -- --ignored fichero_real`. Sólo lee.
    #[test]
    #[ignore = "necesita un localconfig.vdf real (variable STEAM_LOCALCONFIG)"]
    fn fichero_real_ida_y_vuelta() {
        let Ok(path) = std::env::var("STEAM_LOCALCONFIG") else {
            return;
        };
        let original = std::fs::read_to_string(path).unwrap();

        // El fichero puede traer ya una clave (p. ej. una línea pegada a mano).
        let antes = launch_options(&original, "427520").unwrap();
        let nuevo = "\"C:\\x y\\a.exe\" %command%";

        let cambiado = set_launch_options(&original, "427520", Some(nuevo)).unwrap();
        assert_eq!(
            launch_options(&cambiado, "427520").unwrap().as_deref(),
            Some(nuevo)
        );

        // Un fichero de cientos de KB cambia en exactamente una línea: añade una
        // si no había clave, y ninguna si sólo se sustituye el valor.
        let esperadas = original.lines().count() + usize::from(antes.is_none());
        let distintas: Vec<_> = original
            .lines()
            .zip(cambiado.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(cambiado.lines().count(), esperadas, "{distintas:?}");
        assert!(distintas.len() <= 1, "{distintas:?}");

        // Devolver el valor de antes deja el fichero byte a byte como estaba.
        let vuelta = set_launch_options(&cambiado, "427520", antes.as_deref()).unwrap();
        assert_eq!(
            vuelta, original,
            "la ida y vuelta debe devolver el original"
        );
    }

    #[test]
    fn un_bloque_en_una_sola_linea_tambien_se_edita() {
        let plano = "\"apps\"\n{\n\t\"427520\" { \"playtime\" \"1\" }\n}\n";
        let out = set_launch_options(plano, "427520", Some("%command%")).unwrap();
        assert_eq!(
            launch_options(&out, "427520").unwrap().as_deref(),
            Some("%command%")
        );
        // Sin sangría que conservar, basta con que la clave desaparezca.
        let sin = set_launch_options(&out, "427520", None).unwrap();
        assert_eq!(launch_options(&sin, "427520").unwrap(), None);
        assert!(sin.contains("\"playtime\" \"1\""));
    }
}
