use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use super::native_layout::{
    Abstraction, DeclarationSpans, Mark, Member, MemberRecord, Span, TypeHeader, TypeRecord,
    render_globals, render_type,
};
use crate::line_index::{SourcePosition, SourceRange};
use crate::symbols::{
    AccessLevel, Specifier, Specifiers, Symbol, SymbolId, SymbolKind, SymbolOrigin,
};
use crate::types::Type;

include!(concat!(env!("OUT_DIR"), "/native_tables.rs"));

pub(super) const GLOBALS_DOCUMENT: &str = "globals";

struct NativeTypeRow {
    header: TypeHeader<'static>,
    spans: DeclarationSpans,
    members: Range<usize>,
}

struct NativeMemberRow {
    member: Member<'static>,
    spans: DeclarationSpans,
}

struct NativeGlobalRow {
    name: &'static str,
    spans: DeclarationSpans,
}

static TYPE_BY_NAME: LazyLock<HashMap<&'static str, &'static NativeTypeRow>> =
    LazyLock::new(|| {
        NATIVE_TYPES
            .iter()
            .map(|row| (row.header.name, row))
            .collect()
    });

impl NativeTypeRow {
    fn member_rows(&self) -> &'static [NativeMemberRow] {
        &NATIVE_MEMBERS[self.members.clone()]
    }
}

pub(super) fn documents() -> impl Iterator<Item = (&'static str, Vec<Symbol>)> {
    NATIVE_TYPES
        .iter()
        .map(|row| (row.header.name, type_symbols(row)))
        .chain(std::iter::once((GLOBALS_DOCUMENT, global_symbols())))
}

pub(super) fn render(document: &str) -> Option<String> {
    if document == GLOBALS_DOCUMENT {
        return Some(render_globals(
            NATIVE_GLOBALS.iter().map(|row| row.name),
            |_, _| {},
        ));
    }
    let row = TYPE_BY_NAME.get(document)?;
    Some(render_type(
        row.header,
        row.member_rows().iter().map(|member| member.member),
        |_, _| {},
    ))
}

fn type_symbols(row: &NativeTypeRow) -> Vec<Symbol> {
    let header = row.header;
    let mut specifiers = import_only();
    if header.abstraction == Abstraction::Abstract {
        specifiers.insert(Specifier::Abstract);
    }
    let type_id = SymbolId(0);
    let mut symbols = Vec::with_capacity(1 + row.members.len());
    symbols.push(Symbol {
        base_class: header.base.map(str::to_string),
        specifiers,
        ..native_symbol(type_id, type_kind(header.record), header.name, row.spans)
    });
    for member_row in row.member_rows() {
        let member = member_row.member;
        let id = SymbolId(symbols.len());
        let symbol = match member.record {
            MemberRecord::Field => Symbol {
                type_annotation: Some(Type::from_annotation(member.detail)),
                ..native_symbol(id, SymbolKind::Field, member.name, member_row.spans)
            },
            MemberRecord::Method => {
                native_symbol(id, SymbolKind::Method, member.name, member_row.spans)
            }
            MemberRecord::EnumOption => Symbol {
                specifiers: Specifiers::default(),
                ..native_symbol(id, SymbolKind::EnumMember, member.name, member_row.spans)
            },
        };
        symbols.push(Symbol {
            container: Some(type_id),
            container_name: Some(header.name.to_string()),
            ..symbol
        });
    }
    symbols
}

fn global_symbols() -> Vec<Symbol> {
    NATIVE_GLOBALS
        .iter()
        .enumerate()
        .map(|(index, row)| {
            native_symbol(SymbolId(index), SymbolKind::Function, row.name, row.spans)
        })
        .collect()
}

fn native_symbol(id: SymbolId, kind: SymbolKind, name: &str, spans: DeclarationSpans) -> Symbol {
    Symbol {
        id,
        name: name.to_string(),
        kind,
        range: source_range(spans.range),
        selection_range: source_range(spans.selection),
        byte_range: byte_range(spans.range),
        selection_byte_range: byte_range(spans.selection),
        container: None,
        container_name: None,
        type_annotation: None,
        base_class: None,
        owner_class: None,
        flavour: None,
        annotations: Vec::new(),
        access: AccessLevel::Public,
        specifiers: import_only(),
        doc_comment: None,
        origin: SymbolOrigin::Native,
    }
}

fn source_range(span: Span) -> SourceRange {
    SourceRange {
        start: SourcePosition {
            line: span.start.line,
            character: span.start.column,
        },
        end: SourcePosition {
            line: span.end.line,
            character: span.end.column,
        },
    }
}

fn byte_range(span: Span) -> Range<usize> {
    span.start.byte as usize..span.end.byte as usize
}

fn import_only() -> Specifiers {
    let mut specifiers = Specifiers::default();
    specifiers.insert(Specifier::Import);
    specifiers
}

fn type_kind(record: TypeRecord) -> SymbolKind {
    match record {
        TypeRecord::Native => SymbolKind::NativeType,
        TypeRecord::Struct => SymbolKind::Struct,
        TypeRecord::Enum => SymbolKind::Enum,
        TypeRecord::Class => SymbolKind::Class,
    }
}
