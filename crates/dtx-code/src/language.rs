use ast_grep_language::SupportLang;
use std::path::Path;

pub fn detect(path: &Path) -> Option<SupportLang> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some(SupportLang::Rust),
        "ts" | "cts" | "mts" => Some(SupportLang::TypeScript),
        "tsx" => Some(SupportLang::Tsx),
        "js" | "cjs" | "mjs" | "jsx" => Some(SupportLang::JavaScript),
        "py" | "py3" | "pyi" => Some(SupportLang::Python),
        "go" => Some(SupportLang::Go),
        "c" | "h" => Some(SupportLang::C),
        "cc" | "hpp" | "cpp" | "cxx" | "hh" | "cu" => Some(SupportLang::Cpp),
        "java" => Some(SupportLang::Java),
        "rb" | "rbw" | "gemspec" => Some(SupportLang::Ruby),
        "lua" => Some(SupportLang::Lua),
        "swift" => Some(SupportLang::Swift),
        "kt" | "ktm" | "kts" => Some(SupportLang::Kotlin),
        "cs" => Some(SupportLang::CSharp),
        "json" => Some(SupportLang::Json),
        "yaml" | "yml" => Some(SupportLang::Yaml),
        "html" | "htm" | "xhtml" => Some(SupportLang::Html),
        "css" | "scss" => Some(SupportLang::Css),
        "bash" | "sh" | "zsh" => Some(SupportLang::Bash),
        "ex" | "exs" => Some(SupportLang::Elixir),
        "hs" => Some(SupportLang::Haskell),
        "nix" => Some(SupportLang::Nix),
        "php" => Some(SupportLang::Php),
        "scala" | "sc" | "sbt" => Some(SupportLang::Scala),
        "sol" => Some(SupportLang::Solidity),
        "hcl" | "tf" | "tfvars" => Some(SupportLang::Hcl),
        "toml" => None, // not supported by ast-grep
        _ => None,
    }
}
