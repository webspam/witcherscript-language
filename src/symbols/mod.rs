mod extract;
mod types;
mod util;

pub(crate) use extract::SymbolExtractor;
pub use extract::extract_symbols;
pub(crate) use types::enclosing_callable_id;
pub use types::{
    AccessLevel, Annotation, DocumentSymbols, FuncFlavour, Specifier, Specifiers, Symbol, SymbolId,
    SymbolKind,
};
pub use util::node_text;

#[cfg(test)]
mod tests;
