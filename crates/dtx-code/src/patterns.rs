use ast_grep_language::SupportLang;

use crate::symbol::SymbolKind;

fn rust_symbol_kind(node_kind: &str) -> Option<SymbolKind> {
    match node_kind {
        "function_item" | "function_signature_item" => Some(SymbolKind::Function),
        "struct_item" => Some(SymbolKind::Struct),
        "enum_item" => Some(SymbolKind::Enum),
        "trait_item" => Some(SymbolKind::Trait),
        "impl_item" => Some(SymbolKind::Impl),
        "mod_item" => Some(SymbolKind::Module),
        "const_item" => Some(SymbolKind::Constant),
        "static_item" => Some(SymbolKind::Variable),
        "type_item" => Some(SymbolKind::TypeAlias),
        "use_declaration" => Some(SymbolKind::Import),
        _ => None,
    }
}

fn python_symbol_kind(node_kind: &str) -> Option<SymbolKind> {
    match node_kind {
        "function_definition" => Some(SymbolKind::Function),
        "class_definition" => Some(SymbolKind::Class),
        "import_statement" | "import_from_statement" => Some(SymbolKind::Import),
        _ => None,
    }
}

fn typescript_symbol_kind(node_kind: &str) -> Option<SymbolKind> {
    match node_kind {
        "function_declaration" => Some(SymbolKind::Function),
        "class_declaration" => Some(SymbolKind::Class),
        "interface_declaration" => Some(SymbolKind::Interface),
        "enum_declaration" => Some(SymbolKind::Enum),
        "type_alias_declaration" => Some(SymbolKind::TypeAlias),
        "import_statement" => Some(SymbolKind::Import),
        "lexical_declaration" => Some(SymbolKind::Variable),
        "method_definition" => Some(SymbolKind::Method),
        _ => None,
    }
}

fn go_symbol_kind(node_kind: &str) -> Option<SymbolKind> {
    match node_kind {
        "function_declaration" => Some(SymbolKind::Function),
        "method_declaration" => Some(SymbolKind::Method),
        "type_declaration" => Some(SymbolKind::Struct),
        "import_declaration" => Some(SymbolKind::Import),
        _ => None,
    }
}

fn nix_symbol_kind(node_kind: &str) -> Option<SymbolKind> {
    match node_kind {
        "binding" => Some(SymbolKind::Variable),
        "inherit" => Some(SymbolKind::Import),
        _ => None,
    }
}

pub fn symbol_kind_for(lang: SupportLang, node_kind: &str) -> Option<SymbolKind> {
    match lang {
        SupportLang::Rust => rust_symbol_kind(node_kind),
        SupportLang::Python => python_symbol_kind(node_kind),
        SupportLang::TypeScript | SupportLang::Tsx => typescript_symbol_kind(node_kind),
        SupportLang::JavaScript => typescript_symbol_kind(node_kind),
        SupportLang::Go => go_symbol_kind(node_kind),
        SupportLang::Nix => nix_symbol_kind(node_kind),
        _ => None,
    }
}
