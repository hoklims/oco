//! Composite parser that delegates to tree-sitter for supported languages
//! and falls back to regex for everything else.

use std::collections::HashSet;

use crate::error::CodeIntelError;
use crate::parser::{CodeParser, FallbackParser, ParsedFile};
use crate::ts_parser::TreeSitterParser;

/// A parser that uses [`TreeSitterParser`] for languages with grammar support
/// and [`FallbackParser`] for all others.
///
/// This is the recommended parser for production use. It provides accurate
/// AST-based extraction (with `end_line`) for Rust, Python, TypeScript,
/// JavaScript, Go, and Java, while still handling any other language via
/// regex heuristics.
pub struct CompositeParser {
    ts: TreeSitterParser,
    fallback: FallbackParser,
    ts_languages: HashSet<String>,
}

impl CompositeParser {
    /// Create a new composite parser with all tree-sitter grammars loaded.
    pub fn new() -> Self {
        let ts = TreeSitterParser::new();
        let ts_languages: HashSet<String> = ts.supported_languages().into_iter().collect();
        Self {
            ts,
            fallback: FallbackParser::new(),
            ts_languages,
        }
    }
}

impl Default for CompositeParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser for CompositeParser {
    fn parse(&self, source: &str, language: &str) -> Result<ParsedFile, CodeIntelError> {
        if self.ts_languages.contains(language) {
            self.ts.parse(source, language)
        } else {
            self.fallback.parse(source, language)
        }
    }

    fn supported_languages(&self) -> Vec<String> {
        let mut langs: HashSet<String> = self.ts_languages.clone();
        for lang in self.fallback.supported_languages() {
            langs.insert(lang);
        }
        let mut sorted: Vec<String> = langs.into_iter().collect();
        sorted.sort();
        sorted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::SymbolKind;

    #[test]
    fn uses_tree_sitter_for_rust() {
        let parser = CompositeParser::new();
        let result = parser
            .parse("pub fn hello() {}\nfn world() {}", "rust")
            .unwrap();
        // tree-sitter provides end_line, regex does not
        assert!(result.symbols[0].end_line.is_some());
    }

    #[test]
    fn falls_back_for_unknown_language() {
        let parser = CompositeParser::new();
        let result = parser
            .parse("function doSomething() {}", "unknown_lang")
            .unwrap();
        assert!(!result.symbols.is_empty());
        assert_eq!(result.symbols[0].kind, SymbolKind::Function);
    }

    #[test]
    fn supported_languages_is_superset() {
        let parser = CompositeParser::new();
        let langs = parser.supported_languages();
        assert!(langs.contains(&"rust".to_string()));
        assert!(langs.contains(&"python".to_string()));
        assert!(langs.contains(&"go".to_string()));
        assert!(langs.contains(&"java".to_string()));
        assert!(langs.contains(&"typescript".to_string()));
        assert!(langs.contains(&"javascript".to_string()));
    }
}
