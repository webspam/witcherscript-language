#[path = "src/builtins/native_layout.rs"]
mod native_layout;
#[path = "src/builtins/rtti_tags.rs"]
mod rtti_tags;

use std::fmt::Write as _;
use std::ops::Range;
use std::path::Path;

use native_layout::{
    Abstraction, DeclarationSpans, Declared, Mark, Member, MemberRecord, Span, TypeHeader,
    TypeRecord,
};

const RTTI_TABLE: &str = "builtins/rtti.tsv";
const LAYOUT_SOURCE: &str = "src/builtins/native_layout.rs";
const TAGS_SOURCE: &str = "src/builtins/rtti_tags.rs";
const OUTPUT_FILE: &str = "native_tables.rs";

struct ParsedType<'a> {
    header: TypeHeader<'a>,
    members: Vec<Member<'a>>,
}

struct ParsedTable<'a> {
    types: Vec<ParsedType<'a>>,
    globals: Vec<&'a str>,
}

fn main() {
    println!("cargo::rerun-if-changed={RTTI_TABLE}");
    println!("cargo::rerun-if-changed={LAYOUT_SOURCE}");
    println!("cargo::rerun-if-changed={TAGS_SOURCE}");
    let table = std::fs::read_to_string(RTTI_TABLE)
        .unwrap_or_else(|error| panic!("reading {RTTI_TABLE}: {error}"));
    let parsed = parse_table(&table);
    let output_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let output_path = Path::new(&output_dir).join(OUTPUT_FILE);
    std::fs::write(&output_path, emit_tables(&parsed))
        .unwrap_or_else(|error| panic!("writing {}: {error}", output_path.display()));
}

fn parse_table(table: &str) -> ParsedTable<'_> {
    let mut types: Vec<ParsedType> = Vec::new();
    let mut globals = Vec::new();
    for (index, line) in table.lines().enumerate() {
        let line_number = index + 1;
        if line.is_empty() {
            continue;
        }
        let mut columns = line.split('\t');
        let tag = columns.next().unwrap_or_default();
        let name = columns
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| panic!("{RTTI_TABLE}:{line_number}: missing name"));
        match tag {
            rtti_tags::NATIVE => types.push(parse_type(TypeRecord::Native, name, &mut columns)),
            rtti_tags::CLASS => types.push(parse_type(TypeRecord::Class, name, &mut columns)),
            rtti_tags::STRUCT => types.push(parse_type(TypeRecord::Struct, name, &mut columns)),
            rtti_tags::ENUM => types.push(parse_type(TypeRecord::Enum, name, &mut columns)),
            rtti_tags::FIELD => {
                push_member(
                    &mut types,
                    MemberRecord::Field,
                    name,
                    &mut columns,
                    line_number,
                );
            }
            rtti_tags::METHOD => {
                push_member(
                    &mut types,
                    MemberRecord::Method,
                    name,
                    &mut columns,
                    line_number,
                );
            }
            rtti_tags::ENUM_OPTION => {
                push_member(
                    &mut types,
                    MemberRecord::EnumOption,
                    name,
                    &mut columns,
                    line_number,
                );
            }
            rtti_tags::GLOBAL_FUNCTION => globals.push(name),
            _ => panic!("{RTTI_TABLE}:{line_number}: unknown record kind {tag:?}"),
        }
    }
    ParsedTable { types, globals }
}

fn parse_type<'a>(
    record: TypeRecord,
    name: &'a str,
    columns: &mut impl Iterator<Item = &'a str>,
) -> ParsedType<'a> {
    let base = columns.next().filter(|base| !base.is_empty());
    let abstraction = if columns
        .next()
        .is_some_and(|flags| flags.contains(rtti_tags::ABSTRACT_FLAG))
    {
        Abstraction::Abstract
    } else {
        Abstraction::Concrete
    };
    ParsedType {
        header: TypeHeader {
            record,
            name,
            base,
            abstraction,
        },
        members: Vec::new(),
    }
}

fn push_member<'a>(
    types: &mut [ParsedType<'a>],
    record: MemberRecord,
    name: &'a str,
    columns: &mut impl Iterator<Item = &'a str>,
    line_number: usize,
) {
    let owner = types
        .last_mut()
        .unwrap_or_else(|| panic!("{RTTI_TABLE}:{line_number}: member before any type"));
    owner.members.push(Member {
        record,
        name,
        detail: columns.next().unwrap_or_default(),
    });
}

fn emit_tables(parsed: &ParsedTable<'_>) -> String {
    let mut types_out = String::from("static NATIVE_TYPES: &[NativeTypeRow] = &[\n");
    let mut members_out = String::from("static NATIVE_MEMBERS: &[NativeMemberRow] = &[\n");
    let mut member_count = 0usize;
    for parsed_type in &parsed.types {
        let member_rows = member_count..member_count + parsed_type.members.len();
        member_count = member_rows.end;
        native_layout::render_type(
            parsed_type.header,
            parsed_type.members.iter().copied(),
            |declared, spans| match declared {
                Declared::Header => {
                    write_type_row(&mut types_out, &parsed_type.header, &member_rows, spans);
                }
                Declared::Member(index) => {
                    write_member_row(&mut members_out, &parsed_type.members[index], spans);
                }
            },
        );
    }
    types_out.push_str("];\n");
    members_out.push_str("];\n");

    let mut globals_out = String::from("static NATIVE_GLOBALS: &[NativeGlobalRow] = &[\n");
    native_layout::render_globals(parsed.globals.iter().copied(), |index, spans| {
        write_global_row(&mut globals_out, parsed.globals[index], spans);
    });
    globals_out.push_str("];\n");

    types_out + &members_out + &globals_out
}

fn write_type_row(
    out: &mut String,
    header: &TypeHeader<'_>,
    member_rows: &Range<usize>,
    spans: DeclarationSpans,
) {
    let base = match header.base {
        Some(base) => format!("Some({base:?})"),
        None => "None".to_string(),
    };
    let _ = writeln!(
        out,
        "NativeTypeRow {{ header: TypeHeader {{ record: TypeRecord::{:?}, name: {:?}, base: {base}, abstraction: Abstraction::{:?} }}, spans: {}, members: {}..{} }},",
        header.record,
        header.name,
        header.abstraction,
        spans_literal(spans),
        member_rows.start,
        member_rows.end,
    );
}

fn write_member_row(out: &mut String, member: &Member<'_>, spans: DeclarationSpans) {
    let _ = writeln!(
        out,
        "NativeMemberRow {{ member: Member {{ record: MemberRecord::{:?}, name: {:?}, detail: {:?} }}, spans: {} }},",
        member.record,
        member.name,
        member.detail,
        spans_literal(spans),
    );
}

fn write_global_row(out: &mut String, name: &str, spans: DeclarationSpans) {
    let _ = writeln!(
        out,
        "NativeGlobalRow {{ name: {name:?}, spans: {} }},",
        spans_literal(spans)
    );
}

fn spans_literal(spans: DeclarationSpans) -> String {
    format!(
        "DeclarationSpans {{ range: {}, selection: {} }}",
        span_literal(spans.range),
        span_literal(spans.selection)
    )
}

fn span_literal(span: Span) -> String {
    format!(
        "Span {{ start: {}, end: {} }}",
        mark_literal(span.start),
        mark_literal(span.end)
    )
}

fn mark_literal(mark: Mark) -> String {
    format!(
        "Mark {{ byte: {}, line: {}, column: {} }}",
        mark.byte, mark.line, mark.column
    )
}
