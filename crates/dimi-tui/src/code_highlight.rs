//! Shared syntax-highlighting helpers for code previews (port of
//! `apps/dimi/src/tui/components/media/code-highlight.ts`).
//!
//! # Parity gap (documented, not silently hidden)
//!
//! The TS side uses `cli-highlight` (highlight.js) to produce ANSI-highlighted
//! code. There is **no Rust equivalent yet** — a `syntect`-based solution is
//! the planned route but is out of scope for this slice. Consequently
//! [`highlight_lines`] always returns the plain `code.split('\n')` lines, which
//! matches the TS fallback path for unsupported languages / thrown exceptions.
//!
//! [`lang_from_path`] still ports the `EXT_LANG_MAP` and the `supportsLanguage`
//! gate so the API shape is parity-aligned; `supports_language` is an
//! approximation of highlight.js's registered-language list (all `EXT_LANG_MAP`
//! values are verified present in highlight.js 10.7.3).

/// `EXT_LANG_MAP` — file extension → highlight.js language name.
const EXT_LANG_MAP: &[(&str, &str)] = &[
    ("ts", "typescript"),
    ("tsx", "typescript"),
    ("js", "javascript"),
    ("jsx", "javascript"),
    ("py", "python"),
    ("rb", "ruby"),
    ("rs", "rust"),
    ("go", "go"),
    ("java", "java"),
    ("sh", "bash"),
    ("bash", "bash"),
    ("zsh", "bash"),
    ("json", "json"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("toml", "toml"),
    ("md", "markdown"),
    ("css", "css"),
    ("html", "html"),
    ("sql", "sql"),
    ("c", "c"),
    ("cpp", "cpp"),
    ("h", "c"),
    ("hpp", "cpp"),
];

/// Approximates highlight.js's `getLanguage()` gate. Every canonical name in
/// [`EXT_LANG_MAP`] is confirmed present in highlight.js 10.7.3 (the version
/// cli-highlight ships with), plus a handful of common extra languages the TS
/// side would also accept for unmapped extensions. This is the only parity
/// approximation point; the real gate lives behind `cli-highlight`.
fn supports_language(lang: &str) -> bool {
    matches!(
        lang,
        "typescript"
            | "javascript"
            | "python"
            | "ruby"
            | "rust"
            | "go"
            | "java"
            | "bash"
            | "shell"
            | "sh"
            | "zsh"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "markdown"
            | "css"
            | "html"
            | "xml"
            | "sql"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "lua"
            | "perl"
            | "php"
            | "kotlin"
            | "swift"
            | "diff"
            | "makefile"
            | "ini"
            | "plaintext"
    )
}

/// Node `path.extname().slice(1)` equivalent: the substring after the last dot
/// of the basename (a leading dot is not an extension), or empty when there is
/// no extension.
fn extension(file_path: &str) -> String {
    let basename = file_path.rsplit(['/', '\\']).next().unwrap_or(file_path);
    match basename.rfind('.') {
        Some(idx) if idx > 0 => basename[idx + 1..].to_owned(),
        _ => String::new(),
    }
}

/// Resolve a file path to a supported highlight.js language name (port of
/// `langFromPath`). `None` when the path has no extension or the resolved
/// language is not supported.
pub fn lang_from_path(file_path: &str) -> Option<String> {
    let ext = extension(file_path).to_lowercase();
    if ext.is_empty() {
        return None;
    }
    let lang: String = EXT_LANG_MAP
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, l)| (*l).to_owned())
        .unwrap_or(ext);
    if supports_language(&lang) {
        Some(lang)
    } else {
        None
    }
}

/// Highlight `code` for `lang`, returning one ANSI line per code line.
///
/// **Syntax-highlighting parity is NOT implemented** (see module docs): this
/// always returns the plain split lines, which is exactly the TS fallback for
/// unsupported languages / exceptions. Update this function when a
/// syntect-based highlighter lands.
pub fn highlight_lines(code: &str, _lang: Option<&str>) -> Vec<String> {
    code.split('\n').map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_from_path_mapped_extensions() {
        assert_eq!(lang_from_path("src/main.ts").as_deref(), Some("typescript"));
        assert_eq!(lang_from_path("src/App.tsx").as_deref(), Some("typescript"));
        assert_eq!(lang_from_path("script.js").as_deref(), Some("javascript"));
        assert_eq!(lang_from_path("a.py").as_deref(), Some("python"));
        assert_eq!(lang_from_path("lib.rs").as_deref(), Some("rust"));
        assert_eq!(lang_from_path("main.go").as_deref(), Some("go"));
        assert_eq!(lang_from_path("run.sh").as_deref(), Some("bash"));
        assert_eq!(lang_from_path("package.json").as_deref(), Some("json"));
        assert_eq!(lang_from_path("config.yml").as_deref(), Some("yaml"));
        assert_eq!(lang_from_path("Cargo.toml").as_deref(), Some("toml"));
        assert_eq!(lang_from_path("README.md").as_deref(), Some("markdown"));
        assert_eq!(lang_from_path("style.css").as_deref(), Some("css"));
        assert_eq!(lang_from_path("index.html").as_deref(), Some("html"));
        assert_eq!(lang_from_path("query.sql").as_deref(), Some("sql"));
        assert_eq!(lang_from_path("main.c").as_deref(), Some("c"));
        assert_eq!(lang_from_path("main.hpp").as_deref(), Some("cpp"));
    }

    #[test]
    fn lang_from_path_case_and_dots() {
        // Node extname semantics: extension after the LAST dot of the basename.
        assert_eq!(lang_from_path("SRC/Main.TS").as_deref(), Some("typescript"));
        assert_eq!(lang_from_path("a.d.ts").as_deref(), Some("typescript"));
        assert_eq!(lang_from_path(".gitignore").as_deref(), None);
        assert_eq!(lang_from_path("noext").as_deref(), None);
        assert_eq!(lang_from_path("dir/file").as_deref(), None);
    }

    #[test]
    fn lang_from_path_unmapped_extension() {
        // TS: ext not in map → lang = ext; supportsLanguage('xyz') → false.
        assert_eq!(lang_from_path("data.xyz").as_deref(), None);
    }

    #[test]
    fn highlight_lines_falls_back_to_plain_lines() {
        assert_eq!(
            highlight_lines("fn main() {\n  let x = 1;\n}", Some("rust")),
            vec!["fn main() {", "  let x = 1;", "}"]
        );
        assert_eq!(highlight_lines("a\nb", None), vec!["a", "b"]);
    }
}
