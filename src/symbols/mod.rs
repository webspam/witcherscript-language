mod doc;
mod extract;
mod types;
mod util;

pub(crate) use extract::SymbolExtractor;
pub use extract::extract_symbols;
pub use types::{
    AccessLevel, Annotation, DocumentSymbols, FuncFlavour, Specifier, Specifiers, Symbol, SymbolId,
    SymbolKind,
};
pub(crate) use types::{SymbolOrigin, enclosing_callable_id};
pub use util::node_text;

#[cfg(test)]
mod tests;
