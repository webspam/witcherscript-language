//! A request-scoped semantic model of one callable body, queried by the refactor code actions.
//!
//! The CST gives the syntax; this model adds the semantics the refactors keep re-deriving (which
//! identifier resolves to which local, which occurrences read or write it, which definition reaches
//! a read). It is built fresh per request from a parsed snapshot and never cached: an incremental
//! edit invalidates it (invariant 10). Consumers speak only in opaque handles and byte ranges,
//! never tree-sitter nodes, so the backing can change without touching a single caller.

use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::Node;

use crate::cst::ancestors::{find_ancestor_of_kind, node_and_ancestors};
use crate::cst::descendants::{collect_descendants_of_kind, has_descendant_of_kind};
use crate::cst::grammar::write_target;
use crate::cst::nav::{decl_name_idents, first_named_child, named_child_nodes};
use crate::cst::{fields, kinds};
use crate::document::ParsedDocument;
use crate::symbols::{SymbolId, SymbolKind};

use super::Definition;
use super::definition::{definition_key, resolve_definition_at_byte};
use super::extract_common::{
    CALLABLE_KINDS, WriteSite, is_value_type, write_site_node, write_sites,
};
use super::name_context::{NameContext, classify_ident_context};
use super::reaching_defs::reaching_defs;
use super::symbol_db::SymbolDb;

/// Identity of a local, parameter, or field for cross-occurrence matching: `(uri, decl range)`.
type DefKey = (String, Range<usize>);

const SIDE_EFFECT_KINDS: &[&str] = &[kinds::FUNC_CALL_EXPR, kinds::NEW_EXPR];

// Forms that already bind tighter than any surrounding operator, so substituting them needs no parens.
const ATOMIC_VALUE_KINDS: &[&str] = &[
    kinds::IDENT,
    kinds::LITERAL_INT,
    kinds::LITERAL_HEX,
    kinds::LITERAL_FLOAT,
    kinds::LITERAL_BOOL,
    kinds::LITERAL_STRING,
    kinds::LITERAL_NAME,
    kinds::LITERAL_NULL,
    kinds::FUNC_CALL_EXPR,
    kinds::MEMBER_ACCESS_EXPR,
    kinds::ARRAY_EXPR,
    kinds::NESTED_EXPR,
    kinds::NEW_EXPR,
    kinds::THIS_EXPR,
    kinds::PARENT_EXPR,
    kinds::SUPER_EXPR,
    kinds::VIRTUAL_PARENT_EXPR,
];

/// Opaque handle for a local or parameter of the modelled callable. Compare it and pass it back;
/// its internals are deliberately hidden so the identity scheme can change later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LocalId(SymbolId);

/// Whether a value moved to a read is provably unchanged in transit.
pub(crate) enum Stability {
    Stable,
    MayChange,
    /// The value reads the very local being removed.
    ReferencesTarget,
}

#[derive(Clone, Copy)]
pub(crate) enum WriteKinds {
    Reassignment,
    AnyWrite,
}

/// One definition of a local: the value it stores (if substitutable) and the statement that sets it.
pub(crate) struct ReachDef {
    value: Option<Range<usize>>,
    stmt: Option<Range<usize>>,
    is_decl: bool,
}

impl ReachDef {
    pub(crate) fn value(&self) -> Option<Range<usize>> {
        self.value.clone()
    }
    pub(crate) fn stmt(&self) -> Option<Range<usize>> {
        self.stmt.clone()
    }
    pub(crate) fn is_decl(&self) -> bool {
        self.is_decl
    }
}

/// Reaching-definition analysis for one local over the whole body.
pub(crate) struct Reaching {
    per_read: Vec<(Range<usize>, Option<usize>)>,
    defs: Vec<ReachDef>,
}

impl Reaching {
    /// Each read's byte range paired with the index of its sole reaching definition, or `None` when
    /// zero or several reach it.
    pub(crate) fn per_read(&self) -> &[(Range<usize>, Option<usize>)] {
        &self.per_read
    }
    pub(crate) fn defs(&self) -> &[ReachDef] {
        &self.defs
    }
}

pub(crate) struct JoinTarget {
    pub(crate) value: Range<usize>,
    pub(crate) stmt: Range<usize>,
    pub(crate) insert_at: usize,
}

struct LocalEntry {
    id: SymbolId,
    selection: Range<usize>,
}

struct WriteIndex<'a> {
    sites: Vec<(DefKey, WriteSite<'a>)>,
    positions: HashMap<DefKey, Vec<usize>>,
}

pub(crate) struct BodyModel<'a> {
    uri: &'a str,
    document: &'a ParsedDocument,
    db: &'a SymbolDb<'a>,
    body: Node<'a>,
    locals: Vec<LocalEntry>,
    reads_by_local: HashMap<SymbolId, Vec<Range<usize>>>,
    writes: WriteIndex<'a>,
}

impl<'a> BodyModel<'a> {
    /// Build the model for the callable body enclosing `byte`, or `None` outside any callable body.
    pub(crate) fn enclosing(
        uri: &'a str,
        document: &'a ParsedDocument,
        db: &'a SymbolDb<'a>,
        byte: usize,
    ) -> Option<Self> {
        let callable = document.symbols.enclosing_symbol_at(byte, CALLABLE_KINDS)?;
        let root = document.tree.root_node();
        let name = root.descendant_for_byte_range(
            callable.selection_byte_range.start,
            callable.selection_byte_range.end,
        )?;
        let decl = find_ancestor_of_kind(name, &[kinds::FUNC_DECL, kinds::EVENT_DECL])?;
        let body = decl
            .child_by_field_name(fields::DEFINITION)
            .filter(|n| n.kind() == kinds::FUNC_BLOCK)?;

        let locals = document
            .symbols
            .children_of(Some(callable.id))
            .filter(|s| matches!(s.kind, SymbolKind::Variable | SymbolKind::Parameter))
            .map(|s| LocalEntry {
                id: s.id,
                selection: s.selection_byte_range.clone(),
            })
            .collect();

        let reads_by_local = collect_reads(document, callable.id, body);
        let writes = collect_writes(uri, document, db, body);
        Some(BodyModel {
            uri,
            document,
            db,
            body,
            locals,
            reads_by_local,
            writes,
        })
    }

    /// The local whose declaration name covers `byte`, or `None` if `byte` is not on a declaration.
    pub(crate) fn local_declared_at(&self, byte: usize) -> Option<LocalId> {
        self.locals
            .iter()
            .find(|e| e.selection.start <= byte && byte <= e.selection.end)
            .map(|e| LocalId(e.id))
    }

    /// Byte ranges of every occurrence that reads `local`'s value. A whole-value assignment target
    /// (`x = ...`) is not a read; a compound or path target (`x += ...`, `x.f = ...`) is.
    pub(crate) fn reads(&self, local: LocalId) -> &[Range<usize>] {
        self.reads_by_local.get(&local.0).map_or(&[], Vec::as_slice)
    }

    /// Reaching definitions for `local`, with each read mapped to its sole reaching definition.
    pub(crate) fn reaching(&self, local: LocalId) -> Reaching {
        let empty = || Reaching {
            per_read: Vec::new(),
            defs: Vec::new(),
        };
        let Some(decl) = self.decl_node(local) else {
            return empty();
        };
        let key = self.local_key(local);
        let mutations: Vec<&WriteSite> = self
            .writes
            .sites
            .iter()
            .filter(|(k, _)| *k == key)
            .map(|(_, s)| s)
            .collect();
        let mutation_spans: Vec<Range<usize>> = mutations
            .iter()
            .map(|s| write_site_node(s).byte_range())
            .collect();
        // A read that is also a mutation site cannot take the substituted value.
        let reads: Vec<Range<usize>> = self
            .reads(local)
            .iter()
            .filter(|r| !mutation_spans.contains(r))
            .cloned()
            .collect();

        let names_len = decl_name_idents(decl).len();
        let rd = reaching_defs(self.body, decl, names_len, &mutations, &reads);
        let defs = rd
            .all_defs
            .iter()
            .map(|d| ReachDef {
                value: d.value.map(|n| n.byte_range()),
                stmt: d.stmt.map(|n| n.byte_range()),
                is_decl: d.is_decl,
            })
            .collect();
        Reaching {
            per_read: rd.per_read,
            defs,
        }
    }

    pub(crate) fn joinable_assignment(&self, local: LocalId) -> Option<JoinTarget> {
        let decl = self.decl_node(local)?;
        if decl_name_idents(decl).len() != 1
            || decl.child_by_field_name(fields::INIT_VALUE).is_some()
        {
            return None;
        }

        let reaching = self.reaching(local);
        let first = reaching
            .defs()
            .iter()
            .filter(|d| !d.is_decl())
            .min_by_key(|d| d.stmt().map_or(usize::MAX, |s| s.start))?;
        // A compound assignment like `x += 1` has no standalone value to lift into the declaration.
        let value = first.value()?;
        let stmt = first.stmt()?;

        // The declaration and assignment must live in the same block, so nothing branches between them.
        let block = self.node_at(&stmt)?.parent()?;
        if block.kind() != kinds::FUNC_BLOCK || decl.parent().map(|p| p.id()) != Some(block.id()) {
            return None;
        }
        if stmt.start <= decl.end_byte() {
            return None;
        }
        let window = decl.end_byte()..stmt.start;

        // A read before the assignment currently sees the default value, so joining would change it.
        if self.reads(local).iter().any(|r| window.contains(&r.start)) {
            return None;
        }
        if !self.value_hoistable(&value, &window, local, block) {
            return None;
        }
        let insert_at = decl.child_by_field_name(fields::VAR_TYPE)?.end_byte();
        Some(JoinTarget {
            value,
            stmt,
            insert_at,
        })
    }

    fn value_hoistable(
        &self,
        value: &Range<usize>,
        window: &Range<usize>,
        target: LocalId,
        block: Node<'a>,
    ) -> bool {
        let operands: Vec<DefKey> = self
            .referenced_defs(value)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        if operands.contains(&self.local_key(target)) {
            return false;
        }
        // An operand declared between the two is not yet in scope back at the declaration.
        if operands
            .iter()
            .any(|k| k.0 == self.uri && window.contains(&k.1.start))
        {
            return false;
        }
        if self.operand_written_in(value, window, WriteKinds::AnyWrite) {
            return false;
        }
        if self.has_observable_effect(value) {
            // A value with side effects can only move up when nothing runs between the two.
            return !self.has_statement_between(window, block);
        }
        // A call between the two could change one of the operands the value reads.
        !self.window_has_effect(window, block) || operands.is_empty()
    }

    fn statements_between(&self, window: &Range<usize>, block: Node<'a>) -> Vec<Node<'a>> {
        named_child_nodes(block)
            .into_iter()
            .filter(|n| window.start <= n.start_byte() && n.end_byte() <= window.end)
            .collect()
    }

    fn has_statement_between(&self, window: &Range<usize>, block: Node<'a>) -> bool {
        !self.statements_between(window, block).is_empty()
    }

    fn window_has_effect(&self, window: &Range<usize>, block: Node<'a>) -> bool {
        self.statements_between(window, block)
            .into_iter()
            .any(|n| has_descendant_of_kind(n, SIDE_EFFECT_KINDS))
    }

    /// Whether the value at `value` still holds the same result when evaluated at the read site,
    /// given it was captured at `captured_at`.
    pub(crate) fn value_stability(
        &self,
        value: &Range<usize>,
        captured_at: usize,
        target: LocalId,
    ) -> Stability {
        let Some(value_node) = self.node_at(value) else {
            return Stability::MayChange;
        };
        let target_key = self.local_key(target);
        let bytes = self.document.source.as_bytes();
        let mut idents = Vec::new();
        collect_descendants_of_kind(value_node, &[kinds::IDENT], &mut idents);

        let mut stable = true;
        for ident in idents {
            if classify_ident_context(ident, bytes) != Some(NameContext::Value) {
                continue;
            }
            let Some(d) =
                resolve_definition_at_byte(self.uri, self.document, self.db, ident.start_byte())
            else {
                // An unresolved value reference cannot be checked, so the value is not verifiable.
                stable = false;
                continue;
            };
            if !matches!(d.symbol.kind, SymbolKind::Variable | SymbolKind::Parameter) {
                continue;
            }
            let key = definition_key(&d);
            if key == target_key {
                return Stability::ReferencesTarget;
            }
            if self
                .writes
                .positions
                .get(&key)
                .is_some_and(|positions| positions.iter().any(|&p| p >= captured_at))
            {
                stable = false;
            }
        }
        if stable {
            Stability::Stable
        } else {
            Stability::MayChange
        }
    }

    /// Whether substituting the value at `value` into the read at `read` needs wrapping parentheses.
    pub(crate) fn needs_parentheses(&self, value: &Range<usize>, read: &Range<usize>) -> bool {
        let Some(value_node) = self.node_at(value) else {
            return false;
        };
        if ATOMIC_VALUE_KINDS.contains(&value_node.kind()) {
            return false;
        }
        let read_parent = self.node_at(read).and_then(|n| n.parent());
        read_parent.is_some_and(context_binds_tighter)
    }

    /// Whether the statement covering `span` would drop an observable effect if deleted.
    pub(crate) fn has_observable_effect(&self, span: &Range<usize>) -> bool {
        self.node_at(span)
            .is_some_and(|n| has_descendant_of_kind(n, SIDE_EFFECT_KINDS))
    }

    pub(crate) fn local_for(&self, id: SymbolId) -> Option<LocalId> {
        self.locals
            .iter()
            .find(|e| e.id == id)
            .map(|e| LocalId(e.id))
    }

    pub(crate) fn is_written_in(&self, local: LocalId, span: &Range<usize>) -> bool {
        let key = self.local_key(local);
        self.written_in(&key, self.local_is_value_type(local), span)
    }

    pub(crate) fn field_written_in(&self, field: &Definition, span: &Range<usize>) -> bool {
        let value_type = field
            .symbol
            .type_annotation
            .as_ref()
            .is_some_and(|ty| is_value_type(ty, self.db));
        self.written_in(&definition_key(field), value_type, span)
    }

    /// Whether an unconditional whole-value write overwrites `local`'s entry value before any read in `span`.
    pub(crate) fn entry_value_unread_in(
        &self,
        local: LocalId,
        span: &Range<usize>,
        run_block: &Range<usize>,
    ) -> bool {
        let key = self.local_key(local);
        let kill = self
            .writes
            .sites
            .iter()
            .filter(|(k, _)| *k == key)
            .filter_map(|(_, site)| self.unconditional_whole_write_end(site, span, run_block))
            .min();
        let Some(kill) = kill else { return false };
        self.reads(local)
            .iter()
            .filter(|r| span.start <= r.start && r.end <= span.end)
            .all(|r| r.start >= kill)
    }

    pub(crate) fn live_after(&self, local: LocalId, selection: &Range<usize>) -> bool {
        let windows = self.after_windows(selection);
        let hits = |pos: usize| windows.iter().any(|w| w.contains(&pos));
        let read = self.reads(local).iter().any(|r| hits(r.start));
        let key = self.local_key(local);
        let written = self
            .writes
            .positions
            .get(&key)
            .is_some_and(|ps| ps.iter().any(|&p| hits(p)));
        read || written
    }

    pub(crate) fn operand_written_in(
        &self,
        value: &Range<usize>,
        window: &Range<usize>,
        kinds: WriteKinds,
    ) -> bool {
        let operands: Vec<DefKey> = self
            .referenced_defs(value)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        self.writes.sites.iter().any(|(key, site)| {
            let counts = match kinds {
                WriteKinds::Reassignment => {
                    matches!(site, WriteSite::AssignTarget(_) | WriteSite::OutArg(_))
                }
                WriteKinds::AnyWrite => true,
            };
            counts && operands.contains(key) && window.contains(&write_site_node(site).start_byte())
        })
    }

    /// Whether the value at `value` reads a field.
    pub(crate) fn references_field(&self, value: &Range<usize>) -> bool {
        self.referenced_defs(value)
            .iter()
            .any(|(_, kind)| *kind == SymbolKind::Field)
    }

    /// Locals, parameters, and fields the value at `value` references, by definition key and kind.
    fn referenced_defs(&self, value: &Range<usize>) -> Vec<(DefKey, SymbolKind)> {
        let Some(node) = self.node_at(value) else {
            return Vec::new();
        };
        let mut idents = Vec::new();
        collect_descendants_of_kind(node, &[kinds::IDENT], &mut idents);
        idents
            .iter()
            .filter_map(|ident| {
                let d = resolve_definition_at_byte(
                    self.uri,
                    self.document,
                    self.db,
                    ident.start_byte(),
                )?;
                matches!(
                    d.symbol.kind,
                    SymbolKind::Variable | SymbolKind::Parameter | SymbolKind::Field
                )
                .then(|| (definition_key(&d), d.symbol.kind))
            })
            .collect()
    }

    fn written_in(&self, key: &DefKey, value_type: bool, span: &Range<usize>) -> bool {
        self.writes.sites.iter().any(|(k, site)| {
            k == key && span_contains(span, write_site_node(site)) && is_write(site, value_type)
        })
    }

    fn unconditional_whole_write_end(
        &self,
        site: &WriteSite,
        span: &Range<usize>,
        run_block: &Range<usize>,
    ) -> Option<usize> {
        let WriteSite::AssignTarget(node) = site else {
            return None;
        };
        if !span_contains(span, *node) || !is_whole_value_write(*node) {
            return None;
        }
        let stmt = find_ancestor_of_kind(*node, &[kinds::EXPR_STMT])?;
        let parent = stmt.parent()?;
        (parent.start_byte() == run_block.start && parent.end_byte() == run_block.end)
            .then(|| stmt.end_byte())
    }

    fn after_windows(&self, selection: &Range<usize>) -> Vec<Range<usize>> {
        let after = selection.end..self.body.end_byte();
        let mut windows = vec![after];
        // The loop's next iteration runs pre-selection code after the selection, so a read there sees the new value.
        if let Some(loop_node) = self.enclosing_loop(selection) {
            windows.push(loop_node.start_byte()..selection.start);
        }
        windows
    }

    fn enclosing_loop(&self, selection: &Range<usize>) -> Option<Node<'a>> {
        let probe = self
            .body
            .named_descendant_for_byte_range(selection.start, selection.start)?;
        node_and_ancestors(probe)
            .take_while(|n| n.id() != self.body.id())
            .filter(|n| {
                matches!(
                    n.kind(),
                    kinds::FOR_STMT | kinds::WHILE_STMT | kinds::DO_WHILE_STMT
                )
            })
            .last()
    }

    fn local_is_value_type(&self, local: LocalId) -> bool {
        self.document
            .symbols
            .by_id(local.0)
            .and_then(|s| s.type_annotation.as_ref())
            .is_some_and(|ty| is_value_type(ty, self.db))
    }

    fn decl_node(&self, local: LocalId) -> Option<Node<'a>> {
        let entry = self.locals.iter().find(|e| e.id == local.0)?;
        let node = self
            .body
            .descendant_for_byte_range(entry.selection.start, entry.selection.end)?;
        find_ancestor_of_kind(node, &[kinds::LOCAL_VAR_DECL_STMT])
    }

    fn local_key(&self, local: LocalId) -> DefKey {
        let selection = self
            .locals
            .iter()
            .find(|e| e.id == local.0)
            .map_or(0..0, |e| e.selection.clone());
        (self.uri.to_string(), selection)
    }

    fn node_at(&self, span: &Range<usize>) -> Option<Node<'a>> {
        self.body.descendant_for_byte_range(span.start, span.end)
    }
}

fn collect_reads(
    document: &ParsedDocument,
    callable: SymbolId,
    body: Node,
) -> HashMap<SymbolId, Vec<Range<usize>>> {
    let source = &document.source;
    let bytes = source.as_bytes();
    let mut idents = Vec::new();
    collect_descendants_of_kind(body, &[kinds::IDENT], &mut idents);

    let mut reads: HashMap<SymbolId, Vec<Range<usize>>> = HashMap::new();
    for ident in idents {
        if classify_ident_context(ident, bytes) != Some(NameContext::Value) {
            continue;
        }
        if is_whole_value_write(ident) {
            continue;
        }
        let name = &source[ident.byte_range()];
        // Locals shadow fields and globals, so a value-context name match resolves to the local.
        if let Some(local) = document
            .symbols
            .local_at_byte(callable, name, ident.start_byte())
        {
            reads.entry(local.id).or_default().push(ident.byte_range());
        }
    }
    reads
}

fn collect_writes<'a>(
    uri: &str,
    document: &ParsedDocument,
    db: &SymbolDb,
    body: Node<'a>,
) -> WriteIndex<'a> {
    let mut sites = Vec::new();
    let mut positions: HashMap<DefKey, Vec<usize>> = HashMap::new();
    for site in write_sites(uri, document, db, &[body]) {
        let node = write_site_node(&site);
        let Some(d) = resolve_definition_at_byte(uri, document, db, node.start_byte()) else {
            continue;
        };
        let key = definition_key(&d);
        positions
            .entry(key.clone())
            .or_default()
            .push(node.start_byte());
        sites.push((key, site));
    }
    WriteIndex { sites, positions }
}

// `x = ...` where `x` is the entire left-hand side: the prior value is overwritten, not read.
fn is_whole_value_write(ident: Node) -> bool {
    let Some(assign) = find_ancestor_of_kind(ident, &[kinds::ASSIGN_OP_EXPR]) else {
        return false;
    };
    let Some(left) = assign.child_by_field_name(fields::LEFT) else {
        return false;
    };
    if write_target(left).map(|n| n.id()) != Some(ident.id()) {
        return false;
    }
    let direct = assign
        .child_by_field_name(fields::OP)
        .is_some_and(|op| op.kind() == kinds::ASSIGN_OP_DIRECT);
    direct && unwrap_nested(left).id() == ident.id()
}

fn unwrap_nested(expr: Node) -> Node {
    match expr.kind() {
        kinds::NESTED_EXPR => first_named_child(expr).map_or(expr, unwrap_nested),
        _ => expr,
    }
}

fn span_contains(span: &Range<usize>, node: Node) -> bool {
    span.start <= node.start_byte() && node.end_byte() <= span.end
}

fn is_write(site: &WriteSite, value_type: bool) -> bool {
    match site {
        WriteSite::AssignTarget(_) | WriteSite::OutArg(_) => true,
        WriteSite::AssignBase(_) | WriteSite::ReceiverBase(_) => value_type,
    }
}

// In these positions the value is a whole operand, so no outer operator can capture part of it.
fn context_binds_tighter(parent: Node) -> bool {
    !matches!(
        parent.kind(),
        kinds::RETURN_STMT
            | kinds::EXPR_STMT
            | kinds::FUNC_CALL_ARGS
            | kinds::NESTED_EXPR
            | kinds::LOCAL_VAR_DECL_STMT
            | kinds::ASSIGN_OP_EXPR
            | kinds::SWITCH_CASE_LABEL
            | kinds::DELETE_STMT
            | kinds::SEQUENCE_EXPRESSION
    )
}

#[cfg(test)]
mod tests;
