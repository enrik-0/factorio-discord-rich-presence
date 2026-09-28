//! Minimal editing of `LaunchOptions` in Steam's `localconfig.vdf`.
//!
//! This isn't a full VDF parser. All that's needed is to locate an application's
//! block (`apps` → `<id>`) and read, change or remove one of its keys, leaving
//! the rest of the file intact byte for byte: indentation, line breaks and
//! everything else Steam wrote. A 650 KB file with hundreds of blocks isn't
//! reserialized; it's cut and stitched at the exact position.

use anyhow::{bail, Result};

const KEY: &str = "LaunchOptions";

/// A text symbol with its byte positions.
///
/// This works on bytes because `"`, `\`, `{` and `}` are ASCII and never appear
/// inside a multibyte UTF-8 sequence: cutting on them is always valid.
#[derive(Debug, Clone, Copy)]
enum Token {
    /// Quoted string: `start` is the first byte after the opening quote and
    /// `end` is the closing quote (content = `start..end`).
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
                        j += 1; // the escaped character doesn't close the string
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

/// Undoes VDF escaping: `\\` → `\`, `\"` → `"`, `\n`, `\t`.
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

/// Escapes a value for writing it between quotes: `\` → `\\`, `"` → `\"`.
pub fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Position of a block in the token list: its `{` and its `}`.
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

/// Locates `apps` → `<app_id> { … }`.
///
/// The parent is required to be `apps`: the same number appears in other
/// sections of `localconfig.vdf` as a standalone value, and those aren't the
/// application's block.
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
                i += 2; // key and brace
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

/// Where `LaunchOptions` is within a block: the opening quote of the key and
/// the range of the value's content.
struct Found {
    key_quote: usize,
    value_start: usize,
    value_end: usize,
}

/// Looks for the key only among the block's direct children, without going
/// down into sub-blocks (`cloud`, `autocloud`…), which could have keys with
/// the same name.
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

/// Start of the line containing position `pos`.
fn line_start(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

fn app_block(text: &str, tokens: &[Token], app_id: &str) -> Result<Block> {
    match find_app_block(text, tokens, app_id) {
        Some(block) => Ok(block),
        None => bail!(
            "no application block for {app_id} in localconfig.vdf; \
             Steam creates it the first time the game is opened from its library"
        ),
    }
}

/// Current launch options, or `None` if none are defined.
pub fn launch_options(text: &str, app_id: &str) -> Result<Option<String>> {
    let tokens = tokenize(text);
    let block = app_block(text, &tokens, app_id)?;
    Ok(find_launch_options(text, &tokens, &block)
        .map(|found| unescape(&text[found.value_start..found.value_end])))
}

/// Returns the text with `LaunchOptions` changed.
///
/// - `Some(value)`: replaces the value, or adds the key at the end of the block.
/// - `None`: removes the key. If it didn't exist, the text is unchanged.
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

/// Removes the key-value pair, and its whole line if there's nothing else on it.
fn remove_pair(text: &str, found: &Found) -> String {
    let pair_start = found.key_quote;
    let pair_end = found.value_end + 1; // after the value's closing quote

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

/// Adds `"LaunchOptions"  "value"` right before the brace that closes the
/// block, with the indentation of its siblings and the file's line-break style.
fn insert_pair(text: &str, tokens: &[Token], block: &Block, value: &str) -> String {
    let close_pos = match tokens[block.close] {
        Token::Close(pos) => pos,
        _ => unreachable!("a block's closing is always a brace"),
    };
    let nl = newline_style(text);
    let pair = format!("\"{KEY}\"\t\t\"{}\"", escape(value));

    let start_of_line = line_start(text, close_pos);
    let indent_of_close = &text[start_of_line..close_pos];

    if indent_of_close.chars().all(|c| c == ' ' || c == '\t') {
        // The `}` is alone on its line: the pair goes one indent level deeper.
        let line = format!("{indent_of_close}\t{pair}{nl}");
        format!(
            "{}{}{}",
            &text[..start_of_line],
            line,
            &text[start_of_line..]
        )
    } else {
        // `{ ... }` on a single line: it's inserted right next to the brace.
        format!("{} {pair} {}", &text[..close_pos], &text[close_pos..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Structure and indentation copied from a real `localconfig.vdf`, plus a
    /// standalone value `"427520"` in another section, which shouldn't be
    /// confused with the block.
    const SAMPLE: &str = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"427520\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1790455585\"\n\t\t\t\t\t\t\"cloud\"\n\t\t\t\t\t\t{\n\t\t\t\t\t\t\t\"last_sync_state\"\t\t\"synchronized\"\n\t\t\t\t\t\t}\n\t\t\t\t\t\t\"playtime\"\t\t\"54904\"\n\t\t\t\t\t}\n\t\t\t\t\t\"999\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"-otra-app\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n\t\"Otra\"\n\t{\n\t\t\"427520\"\t\t\"3800000004000000968771100100100100860600faee\"\n\t}\n}\n";

    fn with_options(value: &str) -> String {
        SAMPLE.replace(
            "\t\t\t\t\t\t\"playtime\"\t\t\"54904\"\n",
            &format!("\t\t\t\t\t\t\"playtime\"\t\t\"54904\"\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"{value}\"\n"),
        )
    }

    #[test]
    fn without_the_key_there_are_no_options() {
        assert_eq!(launch_options(SAMPLE, "427520").unwrap(), None);
    }

    #[test]
    fn standalone_value_in_another_section_is_not_the_block() {
        // If it got confused, `launch_options` would error out or read something else.
        assert!(launch_options(SAMPLE, "427520").is_ok());
        assert_eq!(
            launch_options(SAMPLE, "999").unwrap().as_deref(),
            Some("-otra-app")
        );
    }

    #[test]
    fn a_missing_application_is_a_clear_error() {
        let err = launch_options(SAMPLE, "12345").unwrap_err().to_string();
        assert!(err.contains("12345"), "{err}");
    }

    #[test]
    fn adding_the_key_respects_indentation_and_the_rest() {
        let out =
            set_launch_options(SAMPLE, "427520", Some("\"C:\\a b\\x.exe\" %command%")).unwrap();

        assert_eq!(
            launch_options(&out, "427520").unwrap().as_deref(),
            Some("\"C:\\a b\\x.exe\" %command%")
        );
        // Indentation of its siblings (6 tabs) and escaping in the file.
        assert!(out.contains(
            "\t\t\t\t\t\t\"LaunchOptions\"\t\t\"\\\"C:\\\\a b\\\\x.exe\\\" %command%\"\n\t\t\t\t\t}\n\t\t\t\t\t\"999\""
        ));
        // Everything else, intact: removing the new line recovers the original.
        assert_eq!(set_launch_options(&out, "427520", None).unwrap(), SAMPLE);
    }

    #[test]
    fn does_not_touch_other_applications_options() {
        let out = set_launch_options(SAMPLE, "427520", Some("%command%")).unwrap();
        assert_eq!(
            launch_options(&out, "999").unwrap().as_deref(),
            Some("-otra-app")
        );
    }

    #[test]
    fn replacing_an_existing_value() {
        let before = with_options("-old");
        let out = set_launch_options(&before, "427520", Some("%command% -new")).unwrap();
        assert_eq!(
            launch_options(&out, "427520").unwrap().as_deref(),
            Some("%command% -new")
        );
        assert_eq!(out.matches("LaunchOptions").count(), 2, "one per app");
    }

    #[test]
    fn removing_the_key_deletes_its_whole_line() {
        let before = with_options("-old");
        let out = set_launch_options(&before, "427520", None).unwrap();
        assert_eq!(out, SAMPLE);
    }

    #[test]
    fn removing_a_nonexistent_key_changes_nothing() {
        assert_eq!(set_launch_options(SAMPLE, "427520", None).unwrap(), SAMPLE);
    }

    #[test]
    fn escaping_is_reversible() {
        let value = "\"C:\\Program Files\\x.exe\" --a=\"b\" %command%";
        assert_eq!(unescape(&escape(value)), value);
    }

    #[test]
    fn respects_windows_line_breaks() {
        let crlf = SAMPLE.replace('\n', "\r\n");
        let out = set_launch_options(&crlf, "427520", Some("%command%")).unwrap();
        assert!(!out.replace("\r\n", "").contains('\n'), "no stray \\n");
        assert!(out.contains("\"LaunchOptions\"\t\t\"%command%\"\r\n"));
        assert_eq!(set_launch_options(&out, "427520", None).unwrap(), crlf);
    }

    #[test]
    fn ignores_a_same_named_key_in_a_subblock() {
        let with_subblock = SAMPLE.replace(
            "\"last_sync_state\"\t\t\"synchronized\"",
            "\"LaunchOptions\"\t\t\"not-me\"",
        );
        assert_eq!(launch_options(&with_subblock, "427520").unwrap(), None);
    }

    #[test]
    fn a_value_with_braces_or_slashes_does_not_break_parsing() {
        let weird = with_options("--x={y} // z");
        assert_eq!(
            launch_options(&weird, "427520").unwrap().as_deref(),
            Some("--x={y} // z")
        );
        assert_eq!(
            launch_options(&weird, "999").unwrap().as_deref(),
            Some("-otra-app")
        );
    }

    #[test]
    fn text_with_accents_is_preserved() {
        let with_accents = SAMPLE.replace("\"Otra\"", "\"Ñandú ó\"");
        let out = set_launch_options(&with_accents, "427520", Some("%command%")).unwrap();
        assert!(out.contains("\"Ñandú ó\""));
    }

    /// Check against a real file, which doesn't fit in the repository:
    /// `STEAM_LOCALCONFIG=<path> cargo test -- --ignored fichero_real`. Read-only.
    #[test]
    #[ignore = "necesita un localconfig.vdf real (variable STEAM_LOCALCONFIG)"]
    fn real_file_round_trip() {
        let Ok(path) = std::env::var("STEAM_LOCALCONFIG") else {
            return;
        };
        let original = std::fs::read_to_string(path).unwrap();

        // The file may already carry a key (e.g. a line pasted by hand).
        let before = launch_options(&original, "427520").unwrap();
        let new_value = "\"C:\\x y\\a.exe\" %command%";

        let changed = set_launch_options(&original, "427520", Some(new_value)).unwrap();
        assert_eq!(
            launch_options(&changed, "427520").unwrap().as_deref(),
            Some(new_value)
        );

        // A file of hundreds of KB changes in exactly one line: it adds one
        // if there was no key, and none if only the value is replaced.
        let expected = original.lines().count() + usize::from(before.is_none());
        let different: Vec<_> = original
            .lines()
            .zip(changed.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(changed.lines().count(), expected, "{different:?}");
        assert!(different.len() <= 1, "{different:?}");

        // Restoring the previous value leaves the file byte for byte as it was.
        let back = set_launch_options(&changed, "427520", before.as_deref()).unwrap();
        assert_eq!(back, original, "the round trip must return the original");
    }

    #[test]
    fn a_single_line_block_is_also_edited() {
        let flat = "\"apps\"\n{\n\t\"427520\" { \"playtime\" \"1\" }\n}\n";
        let out = set_launch_options(flat, "427520", Some("%command%")).unwrap();
        assert_eq!(
            launch_options(&out, "427520").unwrap().as_deref(),
            Some("%command%")
        );
        // With no indentation to preserve, it's enough for the key to disappear.
        let without = set_launch_options(&out, "427520", None).unwrap();
        assert_eq!(launch_options(&without, "427520").unwrap(), None);
        assert!(without.contains("\"playtime\" \"1\""));
    }
}
