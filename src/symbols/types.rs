use std::collections::HashMap;

use crate::line_index::SourceRange;
use crate::types::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub enum AccessLevel {
    Private,
    Protected,
    #[default]
    Public,
}

impl AccessLevel {
    /// `None` for the default `Public`; editors omit the redundant keyword.
    pub fn as_keyword(self) -> Option<&'static str> {
        match self {
            AccessLevel::Private => Some("private"),
            AccessLevel::Protected => Some("protected"),
            AccessLevel::Public => None,
        }
    }
}

/// A function flavour keyword (`func_flavour` in the grammar); at most one per declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FuncFlavour {
    Cleanup,
    Entry,
    Exec,
    Quest,
    Reward,
    Storyscene,
    Timer,
}

impl FuncFlavour {
    pub fn from_keyword(keyword: &str) -> Option<Self> {
        Some(match keyword {
            "cleanup" => Self::Cleanup,
            "entry" => Self::Entry,
            "exec" => Self::Exec,
            "quest" => Self::Quest,
            "reward" => Self::Reward,
            "storyscene" => Self::Storyscene,
            "timer" => Self::Timer,
            _ => return None,
        })
    }

    pub fn as_keyword(self) -> &'static str {
        match self {
            Self::Cleanup => "cleanup",
            Self::Entry => "entry",
            Self::Exec => "exec",
            Self::Quest => "quest",
            Self::Reward => "reward",
            Self::Storyscene => "storyscene",
            Self::Timer => "timer",
        }
    }
}

/// A non-access `specifier` keyword. Access (`private`/`protected`/`public`) is modelled
/// separately as [`AccessLevel`] because it is an ordered visibility, not a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Specifier {
    Import,
    Final,
    Latent,
    Abstract,
    Statemachine,
    Editable,
    Saved,
    Const,
    Inlined,
    Optional,
    Out,
}

impl Specifier {
    pub fn from_keyword(keyword: &str) -> Option<Self> {
        Some(match keyword {
            "import" => Self::Import,
            "final" => Self::Final,
            "latent" => Self::Latent,
            "abstract" => Self::Abstract,
            "statemachine" => Self::Statemachine,
            "editable" => Self::Editable,
            "saved" => Self::Saved,
            "const" => Self::Const,
            "inlined" => Self::Inlined,
            "optional" => Self::Optional,
            "out" => Self::Out,
            _ => return None,
        })
    }

    pub fn as_keyword(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Final => "final",
            Self::Latent => "latent",
            Self::Abstract => "abstract",
            Self::Statemachine => "statemachine",
            Self::Editable => "editable",
            Self::Saved => "saved",
            Self::Const => "const",
            Self::Inlined => "inlined",
            Self::Optional => "optional",
            Self::Out => "out",
        }
    }
}

/// The set of non-access specifiers on one declaration, as a bitset over [`Specifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Specifiers(u16);

impl Specifiers {
    /// Canonical render order; only specifiers actually present are yielded by [`Self::iter`].
    const RENDER_ORDER: [Specifier; 11] = [
        Specifier::Import,
        Specifier::Final,
        Specifier::Latent,
        Specifier::Abstract,
        Specifier::Statemachine,
        Specifier::Editable,
        Specifier::Saved,
        Specifier::Const,
        Specifier::Inlined,
        Specifier::Optional,
        Specifier::Out,
    ];

    pub fn insert(&mut self, specifier: Specifier) {
        self.0 |= 1 << specifier as u16;
    }

    pub fn contains(self, specifier: Specifier) -> bool {
        self.0 & (1 << specifier as u16) != 0
    }

    pub fn iter(self) -> impl Iterator<Item = Specifier> {
        Self::RENDER_ORDER
            .into_iter()
            .filter(move |specifier| self.contains(*specifier))
    }

    pub fn is_optional(self) -> bool {
        self.contains(Specifier::Optional)
    }

    pub fn is_out(self) -> bool {
        self.contains(Specifier::Out)
    }

    pub fn is_abstract(self) -> bool {
        self.contains(Specifier::Abstract)
    }

    pub fn is_state_machine(self) -> bool {
        self.contains(Specifier::Statemachine)
    }
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

    pub fn is_outline(self) -> bool {
        !matches!(self, SymbolKind::Variable | SymbolKind::Parameter)
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
    pub base_class: Option<String>,
    pub owner_class: Option<String>,
    pub flavour: Option<FuncFlavour>,
    pub annotations: Vec<Annotation>,
    pub access: AccessLevel,
    pub specifiers: Specifiers,
    pub doc_comment: Option<String>,
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
