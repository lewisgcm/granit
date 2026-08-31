//! A small internal builder for emitting Nix source with correct, typed
//! escaping and automatic indentation.
//!
//! This exists so that [`crate::nixgen`] reads like a description of the Nix it
//! produces, rather than a wall of `push_str` calls. Crucially, escaping is
//! *position-specific* — a value written into a double-quoted Nix string, an
//! indented `''` string, a shell token, or an attribute-name position each need
//! different treatment. The typed methods here make the correct choice at each
//! call site, preserving the exact semantics that real `nix` evaluation
//! validated.

/// Number of spaces per indent level.
const INDENT: usize = 2;

/// Accumulates Nix source with a current indentation level.
#[derive(Debug, Default)]
pub struct NixWriter {
    buf: String,
    level: usize,
}

impl NixWriter {
    pub fn new() -> Self {
        NixWriter::default()
    }

    /// Consume the writer and return the built string.
    pub fn build(self) -> String {
        self.buf
    }

    /// Increase indentation for the duration of `f`.
    pub fn indented(&mut self, f: impl FnOnce(&mut Self)) {
        self.level += 1;
        f(self);
        self.level -= 1;
    }

    /// Write a full line at the current indentation, followed by a newline.
    pub fn line(&mut self, s: &str) {
        for _ in 0..self.level * INDENT {
            self.buf.push(' ');
        }
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    /// Write a blank line.
    pub fn blank(&mut self) {
        self.buf.push('\n');
    }

    // ---- typed value emitters ----

    /// Emit `name = <value>;` where `value` is already-rendered Nix.
    pub fn assign(&mut self, name: &str, value: &str) {
        self.line(&format!("{} = {};", attr(name), value));
    }

    /// Emit `name = "<escaped string>";`.
    pub fn assign_string(&mut self, name: &str, value: &str) {
        self.line(&format!("{} = {};", attr(name), nix_string(value)));
    }

    /// Emit `name.url = "<escaped>";` (used for flake inputs).
    pub fn assign_url(&mut self, name: &str, url: &str) {
        self.line(&format!("{name}.url = {};", nix_string(url)));
    }
}

// ---------- escaping / rendering helpers ----------
//
// These are `pub(crate)` so nixgen can also use them directly where a raw
// rendered value is needed (e.g. inside a list element).

/// Render a Nix double-quoted string literal, escaping the contents.
pub fn nix_string(s: &str) -> String {
    format!("\"{}\"", escape_nix_dq(s))
}

/// Escape a string for use inside a Nix double-quoted string.
pub fn escape_nix_dq(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape a line for inclusion inside a Nix `''...''` (indented) string.
///
/// We preserve `$` for shell variable use (the user command relies on `$out`,
/// `$GRANIT_DEPENDENCIES`, etc.), so we only protect literal `''` (escaped as
/// `'''`) and `${` sequences (escaped as `''${`) that Nix would otherwise
/// interpret.
pub fn escape_nix_multiline(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\'' && i + 1 < chars.len() && chars[i + 1] == '\'' {
            out.push_str("'''");
            i += 2;
            continue;
        }
        if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
            out.push_str("''${");
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Quote a filename as a single-quoted shell token (for use inside a `''`
/// build script). Embedded single quotes are escaped the shell way: `'\''`.
pub fn shell_single_quote(s: &str) -> String {
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

/// Render an attribute name, quoting if it isn't a bare Nix identifier.
pub fn attr(name: &str) -> String {
    let is_bare = !name.is_empty()
        && name
            .chars()
            .enumerate()
            .all(|(i, c)| c.is_ascii_alphanumeric() || c == '_' || c == '-' || (i > 0 && c == '\''));
    let first_ok = name
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic() || c == '_')
        .unwrap_or(false);
    if is_bare && first_ok {
        name.to_string()
    } else {
        nix_string(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indentation_and_lines() {
        let mut w = NixWriter::new();
        w.line("{");
        w.indented(|w| {
            w.assign_string("name", "hello");
            w.indented(|w| w.line("nested"));
        });
        w.line("}");
        assert_eq!(
            w.build(),
            "{\n  name = \"hello\";\n    nested\n}\n"
        );
    }

    #[test]
    fn dq_escaping() {
        assert_eq!(escape_nix_dq(r#"a"b\c$d"#), r#"a\"b\\c\$d"#);
        assert_eq!(nix_string("x"), "\"x\"");
    }

    #[test]
    fn multiline_escaping_preserves_shell_dollar() {
        // Plain $VAR is preserved; ${...} and '' are escaped.
        assert_eq!(escape_nix_multiline("echo $HOME"), "echo $HOME");
        assert_eq!(escape_nix_multiline("echo ${HOME}"), "echo ''${HOME}");
        assert_eq!(escape_nix_multiline("a '' b"), "a ''' b");
    }

    #[test]
    fn shell_quoting() {
        assert_eq!(shell_single_quote("a.txt"), "'a.txt'");
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn attr_bare_vs_quoted() {
        assert_eq!(attr("hello"), "hello");
        assert_eq!(attr("nodejs_20"), "nodejs_20");
        assert_eq!(attr("with-dash"), "with-dash");
        assert_eq!(attr("1bad"), "\"1bad\"");
        assert_eq!(attr("has space"), "\"has space\"");
    }
}
