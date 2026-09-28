//! Composing the Steam launch line around `%command%`.
//!
//! Steam replaces `%command%` with the game's executable and its arguments.
//! For the application to act as a launcher, it's enough to put it right
//! before it: `"C:\path\factorio-discord-rp.exe" %command%`. If the user
//! already had options, they're respected and the application slips in
//! ahead of `%command%`, without losing them.

const COMMAND: &str = "%command%";

/// The executable's path in quotes, exactly as it appears in the line.
fn quoted(exe: &str) -> String {
    format!("\"{exe}\"")
}

/// Position of `needle` in `haystack` ignoring ASCII case.
///
/// Windows paths are case-insensitive. `to_ascii_lowercase` preserves byte
/// length, so the position is valid for cutting the original.
fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}

/// Name of the executable, used to recognize one of our launchers at a
/// different path.
const APP_FILE: &str = "factorio-discord-rp.exe";

/// Range of one of our launchers already present in the options, whatever
/// its path: from the quote that opens its path up to right before
/// `%command%`. Includes any arguments it carried (`--config …`), which
/// belong to it and not to the game.
///
/// This is the case of someone who pasted the line by hand and then uses
/// the installer: it has to be replaced, not leave one launcher nested
/// inside another.
fn find_previous_launcher(existing: &str) -> Option<(usize, usize)> {
    let lower = existing.to_ascii_lowercase();
    let file = lower.find(&format!("{APP_FILE}\""))?;
    let start = lower[..file].rfind('"')?;
    let command = lower.find(COMMAND)?;
    (command > start).then_some((start, command))
}

/// The full line for the simplest case, with no previous options.
pub fn command_line(exe: &str) -> String {
    format!("{} {COMMAND}", quoted(exe))
}

/// Is the application already in the options?
pub fn is_installed(existing: &str, exe: &str) -> bool {
    find_ignore_case(existing, &quoted(exe)).is_some()
}

/// Options resulting from adding the application to whatever was there.
///
/// - no options → `"exe" %command%`
/// - with `%command%` → prepended to its first occurrence
/// - standalone options (without `%command%`) → `"exe" %command% <options>`,
///   which preserves their effect: Steam would append them after the game
///   the same way
/// - already installed → unchanged
pub fn install(existing: &str, exe: &str) -> String {
    let existing = existing.trim();
    if is_installed(existing, exe) {
        return existing.to_string();
    }

    let ours = quoted(exe);
    if existing.is_empty() {
        return command_line(exe);
    }

    // One of our launchers at a different path (e.g. pasted by hand before) gets replaced.
    if let Some((start, command)) = find_previous_launcher(existing) {
        return format!("{}{ours} {}", &existing[..start], &existing[command..]);
    }

    match existing.find(COMMAND) {
        Some(pos) => format!("{}{ours} {}", &existing[..pos], &existing[pos..]),
        None => format!("{ours} {COMMAND} {existing}"),
    }
}

/// Options resulting from removing the application. An empty string means
/// nothing is left worth keeping: the key must be deleted.
///
/// If the only thing that survives is `%command%` it's treated as empty: it's
/// equivalent to having no options and leaves the file as it was before
/// installing.
pub fn uninstall(existing: &str, exe: &str) -> String {
    let existing = existing.trim();
    let ours = quoted(exe);

    // Our launcher is removed along with its arguments, wherever the executable is;
    // if there's no `%command%` to delimit it, only this installation's exact path.
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

    fn ours() -> String {
        format!("\"{EXE}\"")
    }

    #[test]
    fn no_previous_options_leaves_the_simple_line() {
        assert_eq!(install("", EXE), format!("{} %command%", ours()));
        assert_eq!(install("   ", EXE), command_line(EXE));
    }

    #[test]
    fn the_full_line_carries_the_whole_path_in_quotes() {
        // This is what's shown to the user to paste: it must be self-sufficient.
        let linea = command_line(EXE);
        assert!(linea.starts_with('"') && linea.contains(EXE));
        assert!(linea.ends_with("%command%"));
    }

    #[test]
    fn with_command_it_is_prepended_to_the_first_occurrence() {
        assert_eq!(
            install("%command% -foo", EXE),
            format!("{} %command% -foo", ours())
        );
        assert_eq!(
            install("otro %command%", EXE),
            format!("otro {} %command%", ours())
        );
    }

    #[test]
    fn standalone_options_are_kept_after_the_game() {
        assert_eq!(
            install("--mod-directory X", EXE),
            format!("{} %command% --mod-directory X", ours())
        );
    }

    #[test]
    fn installing_twice_does_not_duplicate() {
        let once = install("%command% -x", EXE);
        assert_eq!(install(&once, EXE), once);
    }

    #[test]
    fn detects_installation_case_insensitively() {
        assert!(is_installed(&command_line(EXE), &EXE.to_uppercase()));
        assert!(!is_installed("%command%", EXE));
    }

    #[test]
    fn uninstalling_returns_what_was_there() {
        for before in [
            "",
            "%command% -x",
            "gamemoderun %command%",
            "otro %command% -y",
        ] {
            let after = uninstall(&install(before, EXE), EXE);
            let expected = if before == "%command%" { "" } else { before };
            assert_eq!(after, expected, "round trip of {before:?}");
        }
    }

    #[test]
    fn uninstalling_the_only_thing_there_was_leaves_it_empty() {
        assert_eq!(uninstall(&command_line(EXE), EXE), "");
    }

    #[test]
    fn standalone_options_end_up_as_command_plus_options() {
        // Not identical to the original, but equivalent for Steam.
        assert_eq!(
            uninstall(&install("--mod-directory X", EXE), EXE),
            "%command% --mod-directory X"
        );
    }

    #[test]
    fn uninstalling_when_not_installed_touches_nothing() {
        assert_eq!(uninstall("%command% -x", EXE), "%command% -x");
    }

    /// The line a user actually had after pasting it by hand (with `--config`).
    const PEGADA_A_MANO: &str = r#""D:\proyectos\factorio discord rich presence\target\release\factorio-discord-rp.exe" --config "D:\proyectos\factorio discord rich presence\config.toml" %command%"#;

    #[test]
    fn a_hand_pasted_launcher_is_replaced_not_nested() {
        assert_eq!(install(PEGADA_A_MANO, EXE), format!("{} %command%", ours()));
    }

    #[test]
    fn the_previous_launcher_is_replaced_keeping_the_users_options() {
        let before = r#""C:\dev\factorio-discord-rp.exe" --config x %command% -foo"#;
        assert_eq!(install(before, EXE), format!("{} %command% -foo", ours()));
    }

    #[test]
    fn uninstalling_removes_the_launcher_from_any_path() {
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
    fn a_path_with_spaces_and_accents_is_treated_the_same() {
        let exe = r"C:\Users\Ñandú Pérez\Mis Juegos\drp.exe";
        assert_eq!(uninstall(&install("%command%", exe), exe), "");
    }
}
