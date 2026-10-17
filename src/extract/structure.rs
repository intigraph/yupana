//! Source-local symbols, call sites and conservative binding evidence.

use crate::types::Symbol;

/// Lexical hint for the AST tier. Unknown preserves conservative candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallBinding {
    /// No supported explicit binding; use conservative name candidates.
    Unqualified,
    /// A lexical definition in this source file.
    Local,
    /// An imported symbol, including an alias or module receiver.
    Imported {
        /// Import's module path, including relative dots.
        module: String,
        /// Original symbol name, before a local alias.
        name: String,
    },
    /// A module binding, only meaningful before resolving an attribute call.
    Module(String),
    /// Rebound, dynamic, wildcard or otherwise unsupported binding.
    Unknown,
}

/// A resolved-by-name call site: `caller` invokes `callee` at `line`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// Name of the enclosing function the call is made from.
    pub caller: String,
    /// Name of the invoked function/method (best-effort, by name).
    pub callee: String,
    /// 1-based line of the call.
    pub line: usize,
    /// Exact enclosing function definition line; avoids duplicate-name callers.
    pub caller_line: usize,
    /// Lexical scope containing the caller definition.
    pub caller_scope: Vec<String>,
    /// Best-effort lexical evidence, never a type-checker guarantee.
    pub binding: CallBinding,
}

/// The structure extracted from one source file: its symbols and call sites.
#[derive(Debug, Clone, Default)]
pub struct FileStructure {
    /// Named symbols defined in the file.
    pub symbols: Vec<Symbol>,
    /// Intra-file call sites (caller/callee by name).
    pub calls: Vec<CallSite>,
    /// Candidate module-name references from `import` / `use` / `include`
    /// declarations. These are the path segments seen in imports (best-effort,
    /// by name — the tree-sitter tier); the exporter resolves them to sibling
    /// modules by matching module stems (§9.2 `bobbin:imports`,
    /// [`crate::types::Tier::TreeSitter`]).
    pub import_refs: Vec<String>,
}

/// Walker's enclosing callable identity, independent of names elsewhere.
#[derive(Clone)]
pub(super) struct CallableScope {
    pub name: String,
    pub line: usize,
    pub scope: Vec<String>,
}
