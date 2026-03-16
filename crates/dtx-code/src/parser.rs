use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_core::{Doc, Node};
use ast_grep_language::SupportLang;

use crate::patterns::symbol_kind_for;
use crate::symbol::{Symbol, SymbolKind};

pub fn parse_source(source: &str, lang: SupportLang) -> Vec<Symbol> {
    let grep = lang.ast_grep(source);
    let root = grep.root();
    let mut symbols = Vec::new();
    collect_symbols(&root, lang, &mut symbols, &[]);
    symbols
}

fn collect_symbols<D: Doc>(
    node: &Node<'_, D>,
    lang: SupportLang,
    symbols: &mut Vec<Symbol>,
    parent_path: &[String],
) {
    for child in node.children() {
        let kind_str = child.kind();
        if let Some(symbol_kind) = symbol_kind_for(lang, &kind_str) {
            let name = extract_name(&child, &symbol_kind);
            let start = child.start_pos();
            let end = child.end_pos();
            let range = child.range();

            let mut name_parts = parent_path.to_vec();
            if !name.is_empty() {
                name_parts.push(name.clone());
            }
            let name_path = name_parts.join("/");

            let mut children = Vec::new();
            if is_container(&symbol_kind) {
                collect_symbols(&child, lang, &mut children, &name_parts);
            }

            symbols.push(Symbol {
                name,
                kind: symbol_kind,
                name_path,
                start_line: start.line(),
                end_line: end.line(),
                start_byte: range.start,
                end_byte: range.end,
                children,
            });
        } else {
            collect_symbols(&child, lang, symbols, parent_path);
        }
    }
}

fn is_container(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Impl
            | SymbolKind::Class
            | SymbolKind::Module
            | SymbolKind::Trait
            | SymbolKind::Interface
    )
}

fn extract_name<D: Doc>(node: &Node<'_, D>, kind: &SymbolKind) -> String {
    if let Some(name_node) = node.field("name") {
        return name_node.text().to_string();
    }

    // Nix bindings use `attrpath` field instead of `name`
    if let Some(attr_node) = node.field("attrpath") {
        return attr_node.text().to_string();
    }

    if matches!(kind, SymbolKind::Impl) {
        if let Some(type_node) = node.field("type") {
            return type_node.text().to_string();
        }
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rust_function() {
        let source = r#"
fn hello() {
    println!("hello");
}

pub fn world(x: i32) -> String {
    x.to_string()
}
"#;
        let symbols = parse_source(source, SupportLang::Rust);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "hello");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
        assert_eq!(symbols[1].name, "world");
    }

    #[test]
    fn parse_rust_struct_with_impl() {
        let source = r#"
struct Foo {
    x: i32,
}

impl Foo {
    fn new() -> Self {
        Self { x: 0 }
    }

    fn get_x(&self) -> i32 {
        self.x
    }
}
"#;
        let symbols = parse_source(source, SupportLang::Rust);
        let structs: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Struct)
            .collect();
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name, "Foo");

        let impls: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Impl)
            .collect();
        assert_eq!(impls.len(), 1);
        assert_eq!(impls[0].children.len(), 2);
        assert_eq!(impls[0].children[0].name, "new");
    }

    #[test]
    fn parse_rust_enum() {
        let source = r#"
enum Color {
    Red,
    Green,
    Blue,
}
"#;
        let symbols = parse_source(source, SupportLang::Rust);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Color");
        assert_eq!(symbols[0].kind, SymbolKind::Enum);
    }

    #[test]
    fn parse_rust_trait() {
        let source = r#"
trait Drawable {
    fn draw(&self);
    fn area(&self) -> f64;
}
"#;
        let symbols = parse_source(source, SupportLang::Rust);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Drawable");
        assert_eq!(symbols[0].kind, SymbolKind::Trait);
        assert_eq!(symbols[0].children.len(), 2);
    }

    #[test]
    fn parse_python_class() {
        let source = r#"
class MyClass:
    def __init__(self):
        self.x = 0

    def get_x(self):
        return self.x
"#;
        let symbols = parse_source(source, SupportLang::Python);
        let classes: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "MyClass");
    }

    #[test]
    fn parse_typescript_function() {
        let source = r#"
function greet(name: string): void {
    console.log(name);
}
"#;
        let symbols = parse_source(source, SupportLang::TypeScript);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "greet");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
    }

    #[test]
    fn language_detection() {
        use std::path::Path;
        assert_eq!(
            crate::language::detect(Path::new("foo.rs")),
            Some(SupportLang::Rust)
        );
        assert_eq!(
            crate::language::detect(Path::new("bar.py")),
            Some(SupportLang::Python)
        );
        assert_eq!(crate::language::detect(Path::new("baz.txt")), None);
    }

    #[test]
    fn name_path_nesting() {
        let source = r#"
impl Foo {
    fn bar() {}
}
"#;
        let symbols = parse_source(source, SupportLang::Rust);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name_path, "Foo");
        assert_eq!(symbols[0].children[0].name_path, "Foo/bar");
    }

    #[test]
    fn parse_nix_bindings() {
        let source = r#"{ foo = 42; bar = x: x + 1; baz = { nested = true; }; }"#;
        let symbols = parse_source(source, SupportLang::Nix);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"foo"), "Expected 'foo' in {names:?}");
        assert!(names.contains(&"bar"), "Expected 'bar' in {names:?}");
        assert!(names.contains(&"baz"), "Expected 'baz' in {names:?}");
        assert!(symbols.iter().all(|s| s.kind == SymbolKind::Variable));
    }

    #[test]
    fn parse_go_function() {
        let source = r#"
package main

func hello() {
    fmt.Println("hello")
}
"#;
        let symbols = parse_source(source, SupportLang::Go);
        let funcs: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "hello");
    }
}
