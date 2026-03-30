//! Tree-sitter based parser for accurate AST-based symbol extraction.
//!
//! Supports Rust, Python, TypeScript, JavaScript, Go, and Java.
//! Falls back to [`UnsupportedLanguage`](crate::error::CodeIntelError::UnsupportedLanguage)
//! for other languages — callers should use [`CompositeParser`](crate::composite_parser::CompositeParser)
//! to automatically delegate to the regex fallback.

use tree_sitter::{Language, Node, Parser, Tree};

use crate::error::CodeIntelError;
use crate::parser::{CodeParser, ParsedFile};
use crate::symbols::{CallEdge, CallEdgeKind, ImportInfo, SymbolInfo, SymbolKind};

/// AST-based parser backed by tree-sitter grammars.
///
/// Provides accurate symbol extraction with correct `end_line` information,
/// proper method vs function distinction (via AST parent context), and
/// multi-line construct handling that regex cannot achieve.
pub struct TreeSitterParser {
    rust_lang: Language,
    python_lang: Language,
    typescript_lang: Language,
    tsx_lang: Language,
    javascript_lang: Language,
    go_lang: Language,
    java_lang: Language,
}

impl TreeSitterParser {
    /// Create a new tree-sitter parser with all supported grammars.
    pub fn new() -> Self {
        Self {
            rust_lang: tree_sitter_rust::LANGUAGE.into(),
            python_lang: tree_sitter_python::LANGUAGE.into(),
            typescript_lang: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tsx_lang: tree_sitter_typescript::LANGUAGE_TSX.into(),
            javascript_lang: tree_sitter_javascript::LANGUAGE.into(),
            go_lang: tree_sitter_go::LANGUAGE.into(),
            java_lang: tree_sitter_java::LANGUAGE.into(),
        }
    }

    fn parse_tree(&self, source: &str, lang: Language) -> Result<Tree, CodeIntelError> {
        let mut parser = Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| CodeIntelError::ParseError(format!("failed to set language: {e}")))?;
        parser
            .parse(source.as_bytes(), None)
            .ok_or_else(|| CodeIntelError::ParseError("tree-sitter returned no tree".into()))
    }
}

impl Default for TreeSitterParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser for TreeSitterParser {
    fn parse(&self, source: &str, language: &str) -> Result<ParsedFile, CodeIntelError> {
        type Extractor = fn(&Tree, &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>);
        let (lang, extractor): (Language, Extractor) = match language {
            "rust" => (self.rust_lang.clone(), extract_rust),
            "python" => (self.python_lang.clone(), extract_python),
            "typescript" => (self.typescript_lang.clone(), extract_typescript),
            "tsx" => (self.tsx_lang.clone(), extract_typescript), // TSX reuses TS extraction
            "javascript" | "jsx" => (self.javascript_lang.clone(), extract_javascript),
            "go" => (self.go_lang.clone(), extract_go),
            "java" => (self.java_lang.clone(), extract_java),
            other => return Err(CodeIntelError::UnsupportedLanguage(other.to_string())),
        };

        let tree = self.parse_tree(source, lang)?;
        let (symbols, imports) = extractor(&tree, source);
        let calls = extract_calls(&tree, source, &symbols);

        Ok(ParsedFile {
            path: String::new(),
            language: language.to_string(),
            symbols,
            imports,
            calls,
            line_count: source.lines().count() as u32,
        })
    }

    fn supported_languages(&self) -> Vec<String> {
        vec![
            "rust".into(),
            "python".into(),
            "typescript".into(),
            "tsx".into(),
            "javascript".into(),
            "jsx".into(),
            "go".into(),
            "java".into(),
        ]
    }
}

// ---------------------------------------------------------------------------
// Utility helpers
// ---------------------------------------------------------------------------

/// Get UTF-8 text of a node.
fn node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

/// 1-based start line.
fn start_line(node: Node) -> u32 {
    node.start_position().row as u32 + 1
}

/// 1-based end line.
fn end_line(node: Node) -> u32 {
    node.end_position().row as u32 + 1
}

/// Find a named child by field name and return its text.
fn field_text<'a>(node: Node, field: &str, source: &'a [u8]) -> Option<&'a str> {
    node.child_by_field_name(field)
        .map(|n| node_text(n, source))
}

/// Walk all descendants, calling `callback` on each node.
fn walk_tree<F>(node: Node, callback: &mut F)
where
    F: FnMut(Node),
{
    callback(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree(child, callback);
    }
}

/// Get signature: the first line of the node's text.
fn first_line_signature(node: Node, source: &[u8]) -> Option<String> {
    let text = node_text(node, source);
    text.lines().next().map(|l| l.trim().to_string())
}

// ---------------------------------------------------------------------------
// Call edge extraction (language-agnostic)
// ---------------------------------------------------------------------------

/// Extract call edges from a parsed tree by walking all `call_expression` nodes
/// and correlating them with the enclosing function/method from `symbols`.
///
/// Works across all tree-sitter grammars because call nodes share common patterns:
/// - Rust/Go/Java/Python: `call_expression` with `function` field
/// - TypeScript/JavaScript: `call_expression` with `function` field
/// - Method calls: `field_expression` / `member_expression` → method name
fn extract_calls(tree: &Tree, source: &str, symbols: &[SymbolInfo]) -> Vec<CallEdge> {
    let src = source.as_bytes();
    let mut calls = Vec::new();

    // Build a sorted list of function ranges for binary search of enclosing function
    let mut func_ranges: Vec<(u32, u32, &str)> = symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        .filter_map(|s| Some((s.line, s.end_line?, s.name.as_str())))
        .collect();
    func_ranges.sort_by_key(|&(start, _, _)| start);

    walk_tree(tree.root_node(), &mut |node| {
        let resolved = match node.kind() {
            // Rust, Go, TS/JS: call_expression with `function` field
            "call_expression" => node
                .child_by_field_name("function")
                .and_then(|func_node| resolve_callee(func_node, src)),
            // Python: `call` node with `function` field
            "call" => node
                .child_by_field_name("function")
                .and_then(|func_node| resolve_callee(func_node, src)),
            // Java: method_invocation has `name` and optional `object` fields
            "method_invocation" => node.child_by_field_name("name").map(|n| {
                let has_object = node.child_by_field_name("object").is_some();
                let kind = if has_object {
                    CallEdgeKind::Member
                } else {
                    CallEdgeKind::Direct
                };
                (node_text(n, src).to_string(), kind)
            }),
            _ => None,
        };

        let Some((callee, kind)) = resolved else {
            return;
        };
        if callee.is_empty() {
            return;
        }

        let confidence = match kind {
            CallEdgeKind::Direct => 1.0,
            CallEdgeKind::Member => 0.9,
            CallEdgeKind::Scoped => 0.85,
            CallEdgeKind::DynamicGuess => 0.5,
        };

        let call_line = start_line(node);
        let call_col = node.start_position().column as u32 + 1;

        // Find the enclosing function by line range
        let caller = find_enclosing_function(&func_ranges, call_line)
            .unwrap_or("<module>")
            .to_string();

        calls.push(CallEdge {
            caller,
            callee,
            line: call_line,
            col: call_col,
            kind,
            confidence,
        });
    });

    calls
}

/// Resolve the callee name and edge kind from a function node in a call expression.
///
/// Returns `(callee_name, CallEdgeKind)`:
/// - Simple calls: `foo()` → ("foo", Direct)
/// - Method calls: `obj.method()` → ("method", Member)
/// - Scoped calls: `module::func()` → ("func", Scoped)
/// - Fallback: full text → (text, DynamicGuess)
fn resolve_callee(node: Node, source: &[u8]) -> Option<(String, CallEdgeKind)> {
    match node.kind() {
        // Direct identifier: foo()
        "identifier" | "type_identifier" => {
            Some((node_text(node, source).to_string(), CallEdgeKind::Direct))
        }
        // Rust: self.method() or obj.method() → field_expression
        "field_expression" => node
            .child_by_field_name("field")
            .map(|n| (node_text(n, source).to_string(), CallEdgeKind::Member)),
        // TS/JS/Python: obj.method() → member_expression
        "member_expression" => node
            .child_by_field_name("property")
            .map(|n| (node_text(n, source).to_string(), CallEdgeKind::Member)),
        // Python: attribute → obj.method
        "attribute" => node
            .child_by_field_name("attribute")
            .map(|n| (node_text(n, source).to_string(), CallEdgeKind::Member)),
        // Rust: path::func() → scoped_identifier
        "scoped_identifier" => node
            .child_by_field_name("name")
            .map(|n| (node_text(n, source).to_string(), CallEdgeKind::Scoped)),
        // Go: pkg.Func() → selector_expression
        "selector_expression" => node
            .child_by_field_name("field")
            .map(|n| (node_text(n, source).to_string(), CallEdgeKind::Scoped)),
        // Fallback: take the full text — low confidence
        _ => {
            let text = node_text(node, source).trim().to_string();
            if text.len() <= 80 && !text.contains('\n') {
                Some((text, CallEdgeKind::DynamicGuess))
            } else {
                None
            }
        }
    }
}

/// Find the innermost enclosing function for a given line number using binary search.
fn find_enclosing_function<'a>(func_ranges: &[(u32, u32, &'a str)], line: u32) -> Option<&'a str> {
    // Find the best (innermost) match — last function whose start ≤ line and end ≥ line
    let mut best: Option<&str> = None;
    let mut best_start = 0;
    for &(start, end, name) in func_ranges {
        if start <= line && end >= line && start >= best_start {
            best = Some(name);
            best_start = start;
        }
        if start > line {
            break; // sorted, no more candidates
        }
    }
    best
}

// ---------------------------------------------------------------------------
// Rust extraction
// ---------------------------------------------------------------------------

fn extract_rust(tree: &Tree, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
    let src = source.as_bytes();
    let mut symbols = Vec::new();
    let mut imports = Vec::new();

    walk_tree(tree.root_node(), &mut |node| {
        if node.is_error() || node.is_missing() {
            return;
        }
        match node.kind() {
            "function_item" => {
                let Some(name) = field_text(node, "name", src) else {
                    return;
                };
                // Method if inside an impl block. Walk up through declaration_list
                // to find impl_item (method) vs mod_item (free function in module).
                let kind = if is_inside_impl(node) {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                let visibility = rust_visibility(node, src);
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind,
                    line: start_line(node),
                    end_line: Some(end_line(node)),
                    signature: first_line_signature(node, src),
                    visibility,
                });
            }
            "struct_item" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Struct,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: rust_visibility(node, src),
                    });
                }
            }
            "enum_item" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Enum,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: rust_visibility(node, src),
                    });
                }
            }
            "trait_item" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Trait,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: rust_visibility(node, src),
                    });
                }
            }
            "type_item" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Type,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: rust_visibility(node, src),
                    });
                }
            }
            "mod_item" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Module,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: rust_visibility(node, src),
                    });
                }
            }
            "const_item" | "static_item" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Constant,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: rust_visibility(node, src),
                    });
                }
            }
            "use_declaration" => {
                let text = node_text(node, src).trim();
                // Strip `use ` prefix and trailing `;`
                let path_str = text
                    .strip_prefix("use ")
                    .unwrap_or(text)
                    .trim_end_matches(';')
                    .trim();

                let (clean_path, items) = if let Some(brace_start) = path_str.find('{') {
                    let prefix = path_str[..brace_start].trim_end_matches("::").to_string();
                    let items_str = &path_str[brace_start + 1..];
                    let items_str = items_str.trim_end_matches('}');
                    let items: Vec<String> = items_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    (prefix, items)
                } else {
                    (path_str.to_string(), Vec::new())
                };

                imports.push(ImportInfo {
                    path: clean_path,
                    items,
                    line: start_line(node),
                });
            }
            _ => {}
        }
    });

    (symbols, imports)
}

fn rust_visibility(node: Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            return Some(node_text(child, source).trim().to_string());
        }
    }
    None
}

/// Check if a Rust function_item is inside an impl block (not a mod block).
fn is_inside_impl(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "impl_item" => return true,
            "mod_item" | "source_file" => return false,
            // declaration_list is the body of both impl and mod — keep walking
            _ => {}
        }
        current = parent.parent();
    }
    false
}

// ---------------------------------------------------------------------------
// Python extraction
// ---------------------------------------------------------------------------

fn extract_python(tree: &Tree, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
    let src = source.as_bytes();
    let mut symbols = Vec::new();
    let mut imports = Vec::new();

    walk_tree(tree.root_node(), &mut |node| {
        if node.is_error() || node.is_missing() {
            return;
        }
        match node.kind() {
            "function_definition" | "async_function_definition" => {
                // For async, the actual function_definition is a child — skip if we'll visit it
                if node.kind() == "async_function_definition" {
                    // We handle it here directly
                }
                let Some(name) = field_text(node, "name", src) else {
                    return;
                };
                // Method if parent is class body
                let kind = if is_inside_class(node) {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                let visibility = if name.starts_with('_') {
                    Some("private".to_string())
                } else {
                    Some("public".to_string())
                };
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind,
                    line: start_line(node),
                    end_line: Some(end_line(node)),
                    signature: first_line_signature(node, src),
                    visibility,
                });
            }
            "class_definition" => {
                if let Some(name) = field_text(node, "name", src) {
                    let visibility = if name.starts_with('_') {
                        Some("private".to_string())
                    } else {
                        Some("public".to_string())
                    };
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Class,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility,
                    });
                }
            }
            "import_statement" => {
                // `import x, y`
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "dotted_name" {
                        imports.push(ImportInfo {
                            path: node_text(child, src).to_string(),
                            items: Vec::new(),
                            line: start_line(node),
                        });
                    }
                    if child.kind() == "aliased_import"
                        && let Some(dotted) = child.child_by_field_name("name")
                    {
                        imports.push(ImportInfo {
                            path: node_text(dotted, src).to_string(),
                            items: Vec::new(),
                            line: start_line(node),
                        });
                    }
                }
            }
            "import_from_statement" => {
                // `from x import y, z`
                let path = field_text(node, "module_name", src)
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let mut items = Vec::new();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "dotted_name"
                        && child.start_byte()
                            > node
                                .child_by_field_name("module_name")
                                .map(|n| n.end_byte())
                                .unwrap_or(0)
                    {
                        items.push(node_text(child, src).to_string());
                    }
                    if child.kind() == "aliased_import"
                        && let Some(name_node) = child.child_by_field_name("name")
                    {
                        items.push(node_text(name_node, src).to_string());
                    }
                }
                if !path.is_empty() || !items.is_empty() {
                    imports.push(ImportInfo {
                        path,
                        items,
                        line: start_line(node),
                    });
                }
            }
            _ => {}
        }
    });

    (symbols, imports)
}

fn is_inside_class(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "class_definition" => return true,
            // block is the class body — keep walking up
            "block" => {}
            // decorated_definition wraps the def — keep walking
            "decorated_definition" => {}
            _ => return false,
        }
        current = parent.parent();
    }
    false
}

// ---------------------------------------------------------------------------
// TypeScript extraction (also used for TSX)
// ---------------------------------------------------------------------------

fn extract_typescript(tree: &Tree, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
    let src = source.as_bytes();
    let mut symbols = Vec::new();
    let mut imports = Vec::new();

    walk_tree(tree.root_node(), &mut |node| {
        if node.is_error() || node.is_missing() {
            return;
        }
        match node.kind() {
            "function_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    let visibility = ts_export_visibility(node);
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Function,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility,
                    });
                }
            }
            "method_definition" => {
                if let Some(name) = field_text(node, "name", src) {
                    let visibility = ts_accessibility(node, src);
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Method,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility,
                    });
                }
            }
            "class_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    let visibility = ts_export_visibility(node);
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Class,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility,
                    });
                }
            }
            "interface_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    let visibility = ts_export_visibility(node);
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Interface,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility,
                    });
                }
            }
            "type_alias_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    let visibility = ts_export_visibility(node);
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Type,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility,
                    });
                }
            }
            "enum_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    let visibility = ts_export_visibility(node);
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Enum,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility,
                    });
                }
            }
            "lexical_declaration" => {
                // `const FOO = ...` or `let bar = ...`
                let visibility = ts_export_visibility(node);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "variable_declarator"
                        && let Some(name) = field_text(child, "name", src)
                    {
                        let decl_keyword = node_text(node, src);
                        let kind = if decl_keyword.starts_with("const") {
                            SymbolKind::Constant
                        } else {
                            SymbolKind::Variable
                        };
                        symbols.push(SymbolInfo {
                            name: name.to_string(),
                            kind,
                            line: start_line(node),
                            end_line: Some(end_line(node)),
                            signature: first_line_signature(node, src),
                            visibility: visibility.clone(),
                        });
                    }
                }
            }
            "import_statement" => {
                ts_extract_import(node, src, &mut imports);
            }
            _ => {}
        }
    });

    (symbols, imports)
}

fn ts_export_visibility(node: Node) -> Option<String> {
    // In tree-sitter-typescript, exported declarations are children of `export_statement`
    if let Some(parent) = node.parent()
        && parent.kind() == "export_statement"
    {
        return Some("export".to_string());
    }
    None
}

fn ts_accessibility(node: Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "accessibility_modifier" {
            return Some(node_text(child, source).to_string());
        }
    }
    None
}

fn ts_extract_import(node: Node, source: &[u8], imports: &mut Vec<ImportInfo>) {
    // `import { a, b } from 'path'` or `import x from 'path'` or `import 'path'`
    let mut path = String::new();
    let mut items = Vec::new();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "string" || child.kind() == "string_literal" {
            let text = node_text(child, source);
            path = text.trim_matches('\'').trim_matches('"').to_string();
        }
        if child.kind() == "import_clause" {
            let mut inner_cursor = child.walk();
            for import_child in child.children(&mut inner_cursor) {
                if import_child.kind() == "named_imports" {
                    let mut named_cursor = import_child.walk();
                    for spec in import_child.children(&mut named_cursor) {
                        if spec.kind() == "import_specifier"
                            && let Some(name) = field_text(spec, "name", source)
                        {
                            items.push(name.to_string());
                        }
                    }
                }
                if import_child.kind() == "identifier" {
                    items.push(node_text(import_child, source).to_string());
                }
            }
        }
    }

    if !path.is_empty() {
        imports.push(ImportInfo {
            path,
            items,
            line: start_line(node),
        });
    }
}

// ---------------------------------------------------------------------------
// JavaScript extraction
// ---------------------------------------------------------------------------

fn extract_javascript(tree: &Tree, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
    let src = source.as_bytes();
    let mut symbols = Vec::new();
    let mut imports = Vec::new();

    walk_tree(tree.root_node(), &mut |node| {
        if node.is_error() || node.is_missing() {
            return;
        }
        match node.kind() {
            "function_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Function,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: ts_export_visibility(node),
                    });
                }
            }
            "method_definition" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Method,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: None,
                    });
                }
            }
            "class_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Class,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: ts_export_visibility(node),
                    });
                }
            }
            "lexical_declaration" => {
                let visibility = ts_export_visibility(node);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "variable_declarator"
                        && let Some(name) = field_text(child, "name", src)
                    {
                        let decl_keyword = node_text(node, src);
                        let kind = if decl_keyword.starts_with("const") {
                            SymbolKind::Constant
                        } else {
                            SymbolKind::Variable
                        };
                        symbols.push(SymbolInfo {
                            name: name.to_string(),
                            kind,
                            line: start_line(node),
                            end_line: Some(end_line(node)),
                            signature: first_line_signature(node, src),
                            visibility: visibility.clone(),
                        });
                    }
                }
            }
            "import_statement" => {
                ts_extract_import(node, src, &mut imports);
            }
            _ => {}
        }
    });

    (symbols, imports)
}

// ---------------------------------------------------------------------------
// Go extraction
// ---------------------------------------------------------------------------

fn extract_go(tree: &Tree, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
    let src = source.as_bytes();
    let mut symbols = Vec::new();
    let mut imports = Vec::new();

    walk_tree(tree.root_node(), &mut |node| {
        if node.is_error() || node.is_missing() {
            return;
        }
        match node.kind() {
            "function_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    let is_exported = name.starts_with(|c: char| c.is_uppercase());
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Function,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: Some(
                            if is_exported { "public" } else { "private" }.to_string(),
                        ),
                    });
                }
            }
            "method_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    let is_exported = name.starts_with(|c: char| c.is_uppercase());
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Method,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: Some(
                            if is_exported { "public" } else { "private" }.to_string(),
                        ),
                    });
                }
            }
            "type_declaration" => {
                // type X struct/interface/...
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "type_spec"
                        && let Some(name) = field_text(child, "name", src)
                    {
                        let is_exported = name.starts_with(|c: char| c.is_uppercase());
                        let kind = child
                            .child_by_field_name("type")
                            .map(|t| match t.kind() {
                                "struct_type" => SymbolKind::Struct,
                                "interface_type" => SymbolKind::Interface,
                                _ => SymbolKind::Type,
                            })
                            .unwrap_or(SymbolKind::Type);
                        symbols.push(SymbolInfo {
                            name: name.to_string(),
                            kind,
                            line: start_line(child),
                            end_line: Some(end_line(child)),
                            signature: first_line_signature(child, src),
                            visibility: Some(
                                if is_exported { "public" } else { "private" }.to_string(),
                            ),
                        });
                    }
                }
            }
            "const_declaration" => {
                go_extract_names(node, src, SymbolKind::Constant, &mut symbols);
            }
            "var_declaration" => {
                go_extract_names(node, src, SymbolKind::Variable, &mut symbols);
            }
            "import_declaration" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "import_spec"
                        && let Some(path_node) = child.child_by_field_name("path")
                    {
                        let path = node_text(path_node, src).trim_matches('"').to_string();
                        imports.push(ImportInfo {
                            path,
                            items: Vec::new(),
                            line: start_line(child),
                        });
                    }
                    // Single import: `import "path"`
                    if child.kind() == "interpreted_string_literal" {
                        let path = node_text(child, src).trim_matches('"').to_string();
                        imports.push(ImportInfo {
                            path,
                            items: Vec::new(),
                            line: start_line(node),
                        });
                    }
                }
            }
            _ => {}
        }
    });

    (symbols, imports)
}

fn go_extract_names(node: Node, source: &[u8], kind: SymbolKind, symbols: &mut Vec<SymbolInfo>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if (child.kind() == "const_spec" || child.kind() == "var_spec")
            && let Some(name_node) = child.child_by_field_name("name")
        {
            let name = node_text(name_node, source).to_string();
            let is_exported = name.starts_with(|c: char| c.is_uppercase());
            symbols.push(SymbolInfo {
                name,
                kind: kind.clone(),
                line: start_line(child),
                end_line: Some(end_line(child)),
                signature: first_line_signature(child, source),
                visibility: Some(if is_exported { "public" } else { "private" }.to_string()),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Java extraction
// ---------------------------------------------------------------------------

fn extract_java(tree: &Tree, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
    let src = source.as_bytes();
    let mut symbols = Vec::new();
    let mut imports = Vec::new();

    walk_tree(tree.root_node(), &mut |node| {
        if node.is_error() || node.is_missing() {
            return;
        }
        match node.kind() {
            "class_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Class,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: java_visibility(node, src),
                    });
                }
            }
            "interface_declaration" | "annotation_type_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Interface,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: java_visibility(node, src),
                    });
                }
            }
            "enum_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Enum,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: java_visibility(node, src),
                    });
                }
            }
            "method_declaration" | "constructor_declaration" => {
                if let Some(name) = field_text(node, "name", src) {
                    symbols.push(SymbolInfo {
                        name: name.to_string(),
                        kind: SymbolKind::Method,
                        line: start_line(node),
                        end_line: Some(end_line(node)),
                        signature: first_line_signature(node, src),
                        visibility: java_visibility(node, src),
                    });
                }
            }
            "import_declaration" => {
                // `import com.example.Foo;`
                let text = node_text(node, src).trim().to_string();
                let path = text
                    .strip_prefix("import ")
                    .unwrap_or(&text)
                    .strip_prefix("static ")
                    .unwrap_or(text.strip_prefix("import ").unwrap_or(&text))
                    .trim_end_matches(';')
                    .trim()
                    .to_string();
                imports.push(ImportInfo {
                    path,
                    items: Vec::new(),
                    line: start_line(node),
                });
            }
            _ => {}
        }
    });

    (symbols, imports)
}

fn java_visibility(node: Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "modifiers" {
            let text = node_text(child, source);
            if text.contains("public") {
                return Some("public".to_string());
            }
            if text.contains("private") {
                return Some("private".to_string());
            }
            if text.contains("protected") {
                return Some("protected".to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ts_parser() -> TreeSitterParser {
        TreeSitterParser::new()
    }

    // -- Rust --

    #[test]
    fn rust_functions_and_methods() {
        let source = r#"
pub fn hello(name: &str) -> String {
    format!("hello {name}")
}

fn private_fn() {}

pub(crate) async fn async_task() {}

impl Server {
    pub fn start(&self) {}
    fn stop(&self) {}
}
"#;
        let result = ts_parser().parse(source, "rust").unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"private_fn"));
        assert!(names.contains(&"async_task"));
        assert!(names.contains(&"start"));
        assert!(names.contains(&"stop"));

        // hello should be Function, start/stop should be Method
        let hello = result.symbols.iter().find(|s| s.name == "hello").unwrap();
        assert_eq!(hello.kind, SymbolKind::Function);
        assert!(hello.visibility.as_deref() == Some("pub"));
        assert!(hello.end_line.is_some()); // tree-sitter provides end_line

        let start = result.symbols.iter().find(|s| s.name == "start").unwrap();
        assert_eq!(start.kind, SymbolKind::Method);
    }

    #[test]
    fn rust_types_and_imports() {
        let source = r#"
use std::collections::HashMap;
use crate::error::CodeIntelError;

pub struct Config {
    name: String,
}

pub trait Parser {
    fn parse(&self);
}

enum Color { Red, Green, Blue }
"#;
        let result = ts_parser().parse(source, "rust").unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Config"));
        assert!(names.contains(&"Parser"));
        assert!(names.contains(&"Color"));

        let config = result.symbols.iter().find(|s| s.name == "Config").unwrap();
        assert_eq!(config.kind, SymbolKind::Struct);
        assert!(config.end_line.is_some());

        assert_eq!(result.imports.len(), 2);
    }

    #[test]
    fn rust_free_functions_in_modules_are_not_methods() {
        let source = r#"
mod utils {
    pub fn helper() {}
    fn internal() {}
}

impl Server {
    pub fn start(&self) {}
}
"#;
        let result = ts_parser().parse(source, "rust").unwrap();

        let helper = result.symbols.iter().find(|s| s.name == "helper").unwrap();
        assert_eq!(
            helper.kind,
            SymbolKind::Function,
            "free fn in mod should be Function"
        );

        let internal = result
            .symbols
            .iter()
            .find(|s| s.name == "internal")
            .unwrap();
        assert_eq!(
            internal.kind,
            SymbolKind::Function,
            "private fn in mod should be Function"
        );

        let start = result.symbols.iter().find(|s| s.name == "start").unwrap();
        assert_eq!(
            start.kind,
            SymbolKind::Method,
            "fn in impl should be Method"
        );
    }

    // -- Python --

    #[test]
    fn python_classes_and_functions() {
        let source = r#"
import os
from pathlib import Path, PurePath

class MyClass:
    def method(self):
        pass

def top_level():
    pass

async def async_handler(request):
    pass
"#;
        let result = ts_parser().parse(source, "python").unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"MyClass"));
        assert!(names.contains(&"method"));
        assert!(names.contains(&"top_level"));
        assert!(names.contains(&"async_handler"));

        let method = result.symbols.iter().find(|s| s.name == "method").unwrap();
        assert_eq!(method.kind, SymbolKind::Method);

        let top = result
            .symbols
            .iter()
            .find(|s| s.name == "top_level")
            .unwrap();
        assert_eq!(top.kind, SymbolKind::Function);

        assert!(!result.imports.is_empty());
    }

    // -- TypeScript --

    #[test]
    fn typescript_symbols() {
        let source = r#"
import { useState } from 'react';

export function greet(name: string): string {
    return `Hello ${name}`;
}

export class UserService {
    async fetchUser(id: number) {}
}

interface Config {
    apiUrl: string;
}

export const API_KEY = "abc";
"#;
        let result = ts_parser().parse(source, "typescript").unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"greet"));
        assert!(names.contains(&"UserService"));
        assert!(names.contains(&"Config"));
        assert!(names.contains(&"API_KEY"));

        let greet = result.symbols.iter().find(|s| s.name == "greet").unwrap();
        assert_eq!(greet.kind, SymbolKind::Function);
        assert!(greet.end_line.is_some());

        assert!(!result.imports.is_empty());
        assert_eq!(result.imports[0].path, "react");
    }

    // -- Go --

    #[test]
    fn go_symbols() {
        let source = r#"
package main

import "fmt"

func main() {
    fmt.Println("hello")
}

func (s *Server) Start() error {
    return nil
}

type Config struct {
    Port int
}
"#;
        let result = ts_parser().parse(source, "go").unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"Start"));
        assert!(names.contains(&"Config"));

        let start = result.symbols.iter().find(|s| s.name == "Start").unwrap();
        assert_eq!(start.kind, SymbolKind::Method);
        assert_eq!(start.visibility.as_deref(), Some("public"));

        let config = result.symbols.iter().find(|s| s.name == "Config").unwrap();
        assert_eq!(config.kind, SymbolKind::Struct);
    }

    // -- Java --

    #[test]
    fn java_symbols() {
        let source = r#"
import java.util.List;

public class UserService {
    private String name;

    public void fetchUser(int id) {}

    private static void helper() {}
}

public interface Repository {
    void save();
}

public enum Color { RED, GREEN, BLUE }
"#;
        let result = ts_parser().parse(source, "java").unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"UserService"));
        assert!(names.contains(&"fetchUser"));
        assert!(names.contains(&"helper"));
        assert!(names.contains(&"Repository"));
        assert!(names.contains(&"Color"));

        let svc = result
            .symbols
            .iter()
            .find(|s| s.name == "UserService")
            .unwrap();
        assert_eq!(svc.kind, SymbolKind::Class);
        assert_eq!(svc.visibility.as_deref(), Some("public"));

        assert_eq!(result.imports.len(), 1);
        assert!(result.imports[0].path.contains("java.util.List"));
    }

    // -- Unsupported language --

    #[test]
    fn unsupported_language_errors() {
        let result = ts_parser().parse("some code", "haskell");
        assert!(result.is_err());
    }

    // -- End line populated --

    #[test]
    fn end_line_is_populated() {
        let source = "pub fn multi_line(\n    x: i32,\n    y: i32,\n) -> bool {\n    true\n}\n";
        let result = ts_parser().parse(source, "rust").unwrap();
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.line, 1);
        assert_eq!(sym.end_line, Some(6));
    }

    // -- Call graph extraction --

    #[test]
    fn rust_call_edges() {
        let source = r#"
fn helper() -> i32 {
    42
}

pub fn main() {
    let x = helper();
    println!("value: {x}");
}
"#;
        let result = ts_parser().parse(source, "rust").unwrap();
        assert!(!result.calls.is_empty(), "should extract call edges");

        let helper_call = result
            .calls
            .iter()
            .find(|c| c.callee == "helper")
            .expect("should find call to helper");
        assert_eq!(helper_call.caller, "main");
    }

    #[test]
    fn rust_method_call_edges() {
        let source = r#"
struct Server;

impl Server {
    fn start(&self) {
        self.init();
    }

    fn init(&self) {}
}

fn run() {
    let s = Server;
    s.start();
}
"#;
        let result = ts_parser().parse(source, "rust").unwrap();

        let init_call = result
            .calls
            .iter()
            .find(|c| c.callee == "init")
            .expect("should find call to init");
        assert_eq!(init_call.caller, "start");
        assert_eq!(init_call.kind, CallEdgeKind::Member); // self.init()
        assert!(init_call.confidence > 0.8);

        let start_call = result
            .calls
            .iter()
            .find(|c| c.callee == "start")
            .expect("should find call to start");
        assert_eq!(start_call.caller, "run");
        assert_eq!(start_call.kind, CallEdgeKind::Member); // s.start()
        assert!(start_call.confidence > 0.8);
    }

    #[test]
    fn python_call_edges() {
        let source = r#"
def helper():
    return 42

def main():
    x = helper()
    print(x)
"#;
        let result = ts_parser().parse(source, "python").unwrap();

        let helper_call = result
            .calls
            .iter()
            .find(|c| c.callee == "helper")
            .expect("should find call to helper");
        assert_eq!(helper_call.caller, "main");
    }

    #[test]
    fn typescript_call_edges() {
        let source = r#"
function greet(name: string): string {
    return format(name);
}

function format(s: string): string {
    return s.toUpperCase();
}
"#;
        let result = ts_parser().parse(source, "typescript").unwrap();

        let format_call = result
            .calls
            .iter()
            .find(|c| c.callee == "format")
            .expect("should find call to format");
        assert_eq!(format_call.caller, "greet");
    }

    #[test]
    fn go_call_edges() {
        let source = r#"
package main

import "fmt"

func helper() int {
    return 42
}

func main() {
    x := helper()
    fmt.Println(x)
}
"#;
        let result = ts_parser().parse(source, "go").unwrap();

        let helper_call = result
            .calls
            .iter()
            .find(|c| c.callee == "helper")
            .expect("should find call to helper");
        assert_eq!(helper_call.caller, "main");

        // fmt.Println → callee should be "Println"
        let println_call = result
            .calls
            .iter()
            .find(|c| c.callee == "Println")
            .expect("should find call to Println");
        assert_eq!(println_call.caller, "main");
    }

    #[test]
    fn calls_outside_function_are_module_level() {
        let source = r#"
fn setup() {}

setup();

fn main() {
    setup();
}
"#;
        let result = ts_parser().parse(source, "rust").unwrap();

        let module_call = result
            .calls
            .iter()
            .find(|c| c.callee == "setup" && c.caller == "<module>");
        let fn_call = result
            .calls
            .iter()
            .find(|c| c.callee == "setup" && c.caller == "main");

        assert!(module_call.is_some(), "should have module-level call");
        assert!(fn_call.is_some(), "should have call inside main");
    }

    #[test]
    fn scoped_rust_call() {
        let source = r#"
fn main() {
    let map = std::collections::HashMap::new();
}
"#;
        let result = ts_parser().parse(source, "rust").unwrap();

        let new_call = result
            .calls
            .iter()
            .find(|c| c.callee == "new")
            .expect("should find call to new via scoped path");
        assert_eq!(new_call.caller, "main");
        assert_eq!(new_call.kind, CallEdgeKind::Scoped); // std::collections::HashMap::new()
        assert!((new_call.confidence - 0.85).abs() < 0.01);
    }

    #[test]
    fn edge_kind_direct_for_simple_call() {
        let source = r#"
fn helper() -> i32 { 42 }

fn main() {
    helper();
}
"#;
        let result = ts_parser().parse(source, "rust").unwrap();
        let call = result
            .calls
            .iter()
            .find(|c| c.callee == "helper")
            .expect("should find direct call");
        assert_eq!(call.kind, CallEdgeKind::Direct);
        assert!((call.confidence - 1.0).abs() < 0.01);
    }
}
