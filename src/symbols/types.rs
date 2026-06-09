use std::collections::HashMap;

use crate::line_index::SourceRange;
use crate::types::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum AccessLevel {
    Private,
    Protected,
    #[default]
    Public,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Class,
    NativeType,
    Struct,
    Enum,
    EnumMember,
    Function,
    Method,
    Field,
    Variable,
    Parameter,
    State,
    Event,
}

impl SymbolKind {
    pub(crate) fn is_type(self) -> bool {
        matches!(
            self,
            SymbolKind::Class
                | SymbolKind::NativeType
                | SymbolKind::Struct
                | SymbolKind::State
                | SymbolKind::Enum
        )
    }

    /// A type that isn't an enum; currently used to filter types (incorrectly)
    pub(crate) fn is_object_type(self) -> bool {
        matches!(
            self,
            SymbolKind::Class | SymbolKind::NativeType | SymbolKind::Struct | SymbolKind::State
        )
    }

    /// States cannot be used for assignments
    pub(crate) fn is_assignable_type(self) -> bool {
        matches!(
            self,
            SymbolKind::Class | SymbolKind::NativeType | SymbolKind::Struct | SymbolKind::Enum
        )
    }

    pub fn is_callable(self) -> bool {
        matches!(
            self,
            SymbolKind::Function | SymbolKind::Event | SymbolKind::Method
        )
    }

    /// There can be multiple instances of this type (and it has members)
    pub(crate) fn is_instantiable(self) -> bool {
        matches!(
            self,
            SymbolKind::Class | SymbolKind::Struct | SymbolKind::State
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub name: String,
    pub argument: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub range: SourceRange,
    pub selection_range: SourceRange,
    pub byte_range: std::ops::Range<usize>,
    pub selection_byte_range: std::ops::Range<usize>,
    pub container: Option<SymbolId>,
    pub container_name: Option<String>,
    pub type_annotation: Option<Type>,
    pub signature: Option<String>,
    pub base_class: Option<String>,
    pub owner_class: Option<String>,
    pub flavour: Option<String>,
    pub annotations: Vec<Annotation>,
    pub access: AccessLevel,
    pub is_optional: bool,
    pub is_out: bool,
    pub is_state_machine: bool,
    pub is_abstract: bool,
}

impl Symbol {
    pub fn display_detail(&self) -> Option<String> {
        match (self.base_class.as_deref(), self.owner_class.as_deref()) {
            (Some(b), Some(o)) => Some(format!("in {o} extends {b}")),
            (Some(b), None) => Some(format!("extends {b}")),
            (None, Some(o)) => Some(format!("in {o}")),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DocumentSymbols {
    symbols: Vec<Symbol>,
    by_start_byte: Vec<SymbolId>,
    top_level_by_name: HashMap<String, Vec<SymbolId>>,
    type_by_name: HashMap<String, Vec<SymbolId>>,
    members_by_container: HashMap<SymbolId, HashMap<String, Vec<SymbolId>>>,
    locals_in_function: HashMap<SymbolId, HashMap<String, Vec<SymbolId>>>,
}

impl DocumentSymbols {
    pub fn all(&self) -> &[Symbol] {
        &self.symbols
    }

    pub fn by_id(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(id.0)
    }

    pub fn children_of(&self, id: Option<SymbolId>) -> impl Iterator<Item = &Symbol> {
        self.symbols
            .iter()
            .filter(move |symbol| symbol.container == id)
    }

    pub fn enclosing_symbol_at(&self, byte_offset: usize, kinds: &[SymbolKind]) -> Option<&Symbol> {
        let upper = self
            .by_start_byte
            .partition_point(|id| self.symbols[id.0].byte_range.start <= byte_offset);
        if upper == 0 {
            return None;
        }
        let mut cursor: Option<SymbolId> = Some(self.by_start_byte[upper - 1]);
        while let Some(id) = cursor {
            let sym = &self.symbols[id.0];
            if byte_offset <= sym.byte_range.end && kinds.contains(&sym.kind) {
                return Some(sym);
            }
            cursor = sym.container;
        }
        None
    }

    pub fn top_level_by_name(&self, name: &str) -> Option<&Symbol> {
        let ids = self.top_level_by_name.get(name)?;
        ids.iter()
            .map(|id| &self.symbols[id.0])
            .find(|s| s.kind != SymbolKind::State)
            .or_else(|| ids.first().map(|id| &self.symbols[id.0]))
    }

    pub fn top_level_by_name_filtered(
        &self,
        name: &str,
        accept: impl Fn(SymbolKind) -> bool,
    ) -> Option<&Symbol> {
        self.top_level_by_name
            .get(name)?
            .iter()
            .map(|id| &self.symbols[id.0])
            .find(|s| accept(s.kind))
    }

    pub fn type_by_name(&self, name: &str) -> Option<&Symbol> {
        let ids = self.type_by_name.get(name)?;
        ids.iter()
            .map(|id| &self.symbols[id.0])
            .find(|s| s.kind != SymbolKind::State)
            .or_else(|| ids.first().map(|id| &self.symbols[id.0]))
    }

    pub fn type_by_name_filtered(
        &self,
        name: &str,
        accept: impl Fn(SymbolKind) -> bool,
    ) -> Option<&Symbol> {
        self.type_by_name
            .get(name)?
            .iter()
            .map(|id| &self.symbols[id.0])
            .find(|s| accept(s.kind))
    }

    pub fn member_of(
        &self,
        container: SymbolId,
        name: &str,
    ) -> impl Iterator<Item = &Symbol> + use<'_> {
        self.members_by_container
            .get(&container)
            .and_then(|by_name| by_name.get(name))
            .map(|ids| ids.iter())
            .into_iter()
            .flatten()
            .map(|id| &self.symbols[id.0])
    }

    pub fn local_at_byte(
        &self,
        function: SymbolId,
        name: &str,
        before_byte: usize,
    ) -> Option<&Symbol> {
        let by_name = self.locals_in_function.get(&function)?;
        let ids = by_name.get(name)?;
        for id in ids.iter().rev() {
            let sym = &self.symbols[id.0];
            if sym.selection_byte_range.start <= before_byte {
                return Some(sym);
            }
        }
        None
    }

    pub(crate) fn push(&mut self, mut symbol: Symbol) -> SymbolId {
        let id = SymbolId(self.symbols.len());
        symbol.id = id;
        self.symbols.push(symbol);
        id
    }

    pub(crate) fn build_indexes(&mut self) {
        let mut by_start: Vec<SymbolId> = (0..self.symbols.len()).map(SymbolId).collect();
        by_start.sort_by_key(|id| self.symbols[id.0].byte_range.start);
        self.by_start_byte = by_start;

        for sym in &self.symbols {
            match sym.container {
                None => {
                    self.top_level_by_name
                        .entry(sym.name.clone())
                        .or_default()
                        .push(sym.id);
                }
                Some(container) => {
                    self.members_by_container
                        .entry(container)
                        .or_default()
                        .entry(sym.name.clone())
                        .or_default()
                        .push(sym.id);
                }
            }
            if sym.kind.is_object_type() {
                self.type_by_name
                    .entry(sym.name.clone())
                    .or_default()
                    .push(sym.id);
            }
        }

        for sym in &self.symbols {
            if !matches!(sym.kind, SymbolKind::Variable | SymbolKind::Parameter) {
                continue;
            }
            let Some(function) = enclosing_callable_id(&self.symbols, sym) else {
                continue;
            };
            self.locals_in_function
                .entry(function)
                .or_default()
                .entry(sym.name.clone())
                .or_default()
                .push(sym.id);
        }
        for by_name in self.locals_in_function.values_mut() {
            for ids in by_name.values_mut() {
                ids.sort_by_key(|id| self.symbols[id.0].selection_byte_range.start);
            }
        }
    }

    /// Builtins ingestion marks native engine types, which have no declaration syntax of their own.
    pub(crate) fn retag_top_level(&mut self, from: SymbolKind, to: SymbolKind) {
        for symbol in &mut self.symbols {
            if symbol.container.is_none() && symbol.kind == from {
                symbol.kind = to;
            }
        }
    }
}

pub(crate) fn enclosing_callable_id(symbols: &[Symbol], sym: &Symbol) -> Option<SymbolId> {
    let mut current = sym.container?;
    loop {
        let owner = symbols.get(current.0)?;
        if owner.kind.is_callable() {
            return Some(current);
        }
        current = owner.container?;
    }
}
