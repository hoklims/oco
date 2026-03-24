//! Symbol indexer for building and querying a cross-file symbol index.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::warn;

use crate::error::CodeIntelError;
use crate::languages::language_from_path;
use crate::parser::CodeParser;
use crate::parser::FallbackParser;
use crate::symbols::SymbolInfo;

/// In-memory symbol index built by parsing files with [`FallbackParser`].
///
/// Stores symbols keyed by file path for cross-file lookup and reference search.
pub struct SymbolIndexer {
    parser: FallbackParser,
    /// Map from file path to its extracted symbols.
    index: HashMap<String, Vec<SymbolInfo>>,
    /// Map from file path to its raw content (kept for reference search).
    contents: HashMap<String, String>,
}

impl SymbolIndexer {
    /// Create a new empty indexer.
    pub fn new() -> Self {
        Self {
            parser: FallbackParser::new(),
            index: HashMap::new(),
            contents: HashMap::new(),
        }
    }

    /// Parse a single file's content and add its symbols to the index.
    ///
    /// The `language` parameter should be a language name (e.g. `"rust"`, `"python"`).
    /// Returns the extracted symbols.
    pub fn index_file(
        &mut self,
        path: &str,
        content: &str,
        language: &str,
    ) -> Result<Vec<SymbolInfo>, CodeIntelError> {
        let parsed = self.parser.parse(content, language)?;
        let symbols = parsed.symbols;
        self.index.insert(path.to_string(), symbols.clone());
        self.contents.insert(path.to_string(), content.to_string());
        Ok(symbols)
    }

    /// Recursively index all files in a directory that match the given extensions.
    ///
    /// Extensions should NOT include the leading dot (e.g. `["rs", "py"]`).
    /// Returns a map from file path to extracted symbols.
    pub fn index_directory(
        &mut self,
        dir: &str,
        extensions: &[&str],
    ) -> Result<HashMap<String, Vec<SymbolInfo>>, CodeIntelError> {
        let mut results = HashMap::new();
        self.walk_dir(Path::new(dir), extensions, &mut results)?;
        Ok(results)
    }

    /// Search the index for symbols matching the given name (case-sensitive substring match).
    ///
    /// Returns a list of `(file_path, symbol)` pairs.
    pub fn find_symbol(&self, name: &str) -> Vec<(String, SymbolInfo)> {
        let mut matches = Vec::new();
        for (path, symbols) in &self.index {
            for sym in symbols {
                if sym.name == name {
                    matches.push((path.clone(), sym.clone()));
                }
            }
        }
        matches
    }

    /// Find references to a symbol name across all indexed files.
    ///
    /// Performs a simple text search (grep-like) on the stored file contents.
    /// Returns a list of `(file_path, line_number)` pairs.
    pub fn find_references(&self, symbol_name: &str) -> Vec<(String, u32)> {
        let mut refs = Vec::new();
        for (path, content) in &self.contents {
            for (i, line) in content.lines().enumerate() {
                if line.contains(symbol_name) {
                    refs.push((path.clone(), (i + 1) as u32));
                }
            }
        }
        refs
    }

    /// Return a reference to the full index.
    pub fn index(&self) -> &HashMap<String, Vec<SymbolInfo>> {
        &self.index
    }

    /// Return the number of indexed files.
    pub fn file_count(&self) -> usize {
        self.index.len()
    }

    /// Return the total number of symbols across all indexed files.
    pub fn symbol_count(&self) -> usize {
        self.index.values().map(|v| v.len()).sum()
    }

    // -- private helpers --

    fn walk_dir(
        &mut self,
        dir: &Path,
        extensions: &[&str],
        results: &mut HashMap<String, Vec<SymbolInfo>>,
    ) -> Result<(), CodeIntelError> {
        if !dir.is_dir() {
            return Err(CodeIntelError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("directory not found: {}", dir.display()),
            )));
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            // Skip hidden directories and common non-source dirs
            if path.is_dir() {
                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if dir_name.starts_with('.')
                    || dir_name == "node_modules"
                    || dir_name == "target"
                    || dir_name == "__pycache__"
                    || dir_name == "vendor"
                    || dir_name == "dist"
                    || dir_name == "build"
                {
                    continue;
                }
                self.walk_dir(&path, extensions, results)?;
                continue;
            }

            // Check extension
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !extensions.contains(&ext) {
                continue;
            }

            let path_str = path.to_string_lossy().to_string();
            let language = language_from_path(&path_str).unwrap_or("unknown");

            match fs::read_to_string(&path) {
                Ok(content) => match self.index_file(&path_str, &content, language) {
                    Ok(symbols) => {
                        results.insert(path_str, symbols);
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "failed to parse file");
                    }
                },
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "failed to read file");
                }
            }
        }

        Ok(())
    }
}

impl Default for SymbolIndexer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_and_find_symbol() {
        let mut indexer = SymbolIndexer::new();

        let rust_source = r#"
pub fn hello() {}
pub fn world() {}
"#;
        indexer
            .index_file("src/lib.rs", rust_source, "rust")
            .unwrap();

        let matches = indexer.find_symbol("hello");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "src/lib.rs");
        assert_eq!(matches[0].1.name, "hello");
    }

    #[test]
    fn find_references_across_files() {
        let mut indexer = SymbolIndexer::new();

        indexer
            .index_file(
                "src/a.rs",
                "pub fn greet() {}\nfn main() { greet(); }",
                "rust",
            )
            .unwrap();
        indexer
            .index_file("src/b.rs", "use crate::greet;\ngreet();", "rust")
            .unwrap();

        let refs = indexer.find_references("greet");
        // "greet" appears in both files on multiple lines
        assert!(refs.len() >= 3);
    }

    #[test]
    fn counts() {
        let mut indexer = SymbolIndexer::new();
        indexer
            .index_file("a.py", "def foo(): pass\ndef bar(): pass", "python")
            .unwrap();
        indexer
            .index_file("b.py", "class Baz: pass", "python")
            .unwrap();

        assert_eq!(indexer.file_count(), 2);
        assert_eq!(indexer.symbol_count(), 3);
    }

    #[test]
    fn index_directory_nonexistent() {
        let mut indexer = SymbolIndexer::new();
        let result = indexer.index_directory("/nonexistent_dir_xyz", &["rs"]);
        assert!(result.is_err());
    }
}
