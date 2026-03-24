//! Language detection from file extensions.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Map of file extensions to language names.
static EXTENSION_MAP: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // Rust
    m.insert("rs", "rust");
    // TypeScript / JavaScript
    m.insert("ts", "typescript");
    m.insert("tsx", "typescript");
    m.insert("js", "javascript");
    m.insert("jsx", "javascript");
    m.insert("mjs", "javascript");
    m.insert("cjs", "javascript");
    // Python
    m.insert("py", "python");
    m.insert("pyi", "python");
    // Go
    m.insert("go", "go");
    // Java
    m.insert("java", "java");
    // Kotlin
    m.insert("kt", "kotlin");
    m.insert("kts", "kotlin");
    // C / C++
    m.insert("c", "c");
    m.insert("h", "c");
    m.insert("cpp", "cpp");
    m.insert("hpp", "cpp");
    m.insert("cc", "cpp");
    m.insert("cxx", "cpp");
    // C#
    m.insert("cs", "csharp");
    // Ruby
    m.insert("rb", "ruby");
    // PHP
    m.insert("php", "php");
    // Swift
    m.insert("swift", "swift");
    // Scala
    m.insert("scala", "scala");
    // Dart
    m.insert("dart", "dart");
    // Lua
    m.insert("lua", "lua");
    // Shell
    m.insert("sh", "bash");
    m.insert("bash", "bash");
    m.insert("zsh", "bash");
    // Markup / Config
    m.insert("html", "html");
    m.insert("htm", "html");
    m.insert("css", "css");
    m.insert("scss", "scss");
    m.insert("json", "json");
    m.insert("yaml", "yaml");
    m.insert("yml", "yaml");
    m.insert("toml", "toml");
    m.insert("xml", "xml");
    m.insert("md", "markdown");
    // Zig
    m.insert("zig", "zig");
    // Elixir
    m.insert("ex", "elixir");
    m.insert("exs", "elixir");
    // Haskell
    m.insert("hs", "haskell");
    // SQL
    m.insert("sql", "sql");
    // Svelte / Vue
    m.insert("svelte", "svelte");
    m.insert("vue", "vue");
    m
});

/// Detect the language name from a file extension (without the leading dot).
///
/// Returns `None` if the extension is not recognized.
pub fn language_from_extension(ext: &str) -> Option<&'static str> {
    EXTENSION_MAP.get(ext.to_lowercase().as_str()).copied()
}

/// Detect the language name from a full file path.
///
/// Extracts the extension and looks it up.
pub fn language_from_path(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?;
    language_from_extension(ext)
}

/// Return a list of all recognized file extensions.
pub fn known_extensions() -> Vec<&'static str> {
    EXTENSION_MAP.keys().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_common_languages() {
        assert_eq!(language_from_extension("rs"), Some("rust"));
        assert_eq!(language_from_extension("ts"), Some("typescript"));
        assert_eq!(language_from_extension("py"), Some("python"));
        assert_eq!(language_from_extension("go"), Some("go"));
        assert_eq!(language_from_extension("java"), Some("java"));
        assert_eq!(language_from_extension("js"), Some("javascript"));
    }

    #[test]
    fn detect_from_path() {
        assert_eq!(language_from_path("src/main.rs"), Some("rust"));
        assert_eq!(language_from_path("lib/utils.ts"), Some("typescript"));
        assert_eq!(language_from_path("app.py"), Some("python"));
    }

    #[test]
    fn unknown_extension() {
        assert_eq!(language_from_extension("xyz"), None);
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(language_from_extension("RS"), Some("rust"));
        assert_eq!(language_from_extension("Py"), Some("python"));
    }
}
