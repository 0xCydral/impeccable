//! How printed commands spell the Windows `.cmd` launcher (issue #744).
//!
//! `IMPECCABLE_SELF` is the launcher path the binary names in the commands it
//! prints. When that path is the `.cmd` launcher and the printed spelling is
//! quoted, PowerShell parses `"<path>" <verb>` as a string expression and
//! fails with `Unexpected token '<verb>'`. The call operator, `& "<path>"
//! <verb>`, runs. This mirrors `quoteLauncher` in the build's
//! `scripts/lib/plugin-paths.js`.
//!
//! The rule keys on the `.cmd` suffix, not on the OS: only `impeccable.cmd`
//! exports a `.cmd` self path, and it only runs on a Windows shell without
//! sh. The sh launcher, the npm shim's `npx impeccable`, and unquoted paths
//! keep their spelling.

/// True when `value` names the Windows `.cmd` launcher.
pub fn is_cmd_launcher(value: &str) -> bool {
    let value = value.trim();
    value.len() > 4 && value[value.len() - 4..].eq_ignore_ascii_case(".cmd")
}

/// Prefix a quoted spelling of the `.cmd` launcher with PowerShell's call
/// operator. `raw` is the unquoted self path, `rendered` the spelling the
/// caller already quoted for printing. Anything else comes back unchanged.
pub fn with_call_operator(raw: &str, rendered: String) -> String {
    if is_cmd_launcher(raw) && rendered.starts_with('"') {
        format!("& {rendered}")
    } else {
        rendered
    }
}

/// The printed spelling for sites that print the self path as is. A `.cmd`
/// path containing whitespace splits unquoted in every Windows shell, so it
/// is printed as `& "<path>"`; every other value is returned unchanged.
pub fn spell_self(raw: &str) -> String {
    let value = raw.trim();
    if is_cmd_launcher(value) && value.chars().any(char::is_whitespace) && !value.starts_with('"') && !value.starts_with('&') {
        format!("& \"{value}\"")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_only_the_cmd_launcher() {
        assert!(is_cmd_launcher(r"C:\Users\A B\skill\scripts\impeccable.cmd"));
        assert!(is_cmd_launcher("C:/skill/scripts/IMPECCABLE.CMD"));
        assert!(!is_cmd_launcher("/Users/a b/skill/scripts/impeccable"));
        assert!(!is_cmd_launcher("npx impeccable"));
        assert!(!is_cmd_launcher(".cmd"));
    }

    #[test]
    fn quoted_cmd_gets_the_call_operator() {
        let raw = r"C:\Users\A B\impeccable.cmd";
        assert_eq!(with_call_operator(raw, format!("\"{raw}\"")), format!("& \"{raw}\""));
        // Unquoted, or the sh launcher quoted for sh: unchanged.
        assert_eq!(with_call_operator(r"C:\x\impeccable.cmd", r"C:\x\impeccable.cmd".into()), r"C:\x\impeccable.cmd");
        assert_eq!(with_call_operator("/a b/impeccable", "'/a b/impeccable'".into()), "'/a b/impeccable'");
        assert_eq!(with_call_operator("/a b/impeccable", "\"/a b/impeccable\"".into()), "\"/a b/impeccable\"");
    }

    #[test]
    fn raw_spelling_quotes_only_a_spaced_cmd_path() {
        assert_eq!(spell_self(r"C:\Users\A B\impeccable.cmd"), r#"& "C:\Users\A B\impeccable.cmd""#);
        assert_eq!(spell_self(r"C:\skill\impeccable.cmd"), r"C:\skill\impeccable.cmd");
        assert_eq!(spell_self("/Users/a b/skill/scripts/impeccable"), "/Users/a b/skill/scripts/impeccable");
        assert_eq!(spell_self("npx impeccable"), "npx impeccable");
        // Already spelled: never double-wrapped.
        let spelled = spell_self(r"C:\A B\impeccable.cmd");
        assert_eq!(spell_self(&spelled), spelled);
    }
}
