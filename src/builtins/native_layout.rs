#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeRecord {
    Native,
    Class,
    Struct,
    Enum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberRecord {
    Field,
    Method,
    EnumOption,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Abstraction {
    Abstract,
    Concrete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mark {
    pub(crate) byte: u32,
    pub(crate) line: u32,
    pub(crate) column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) start: Mark,
    pub(crate) end: Mark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeclarationSpans {
    pub(crate) range: Span,
    pub(crate) selection: Span,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TypeHeader<'a> {
    pub(crate) record: TypeRecord,
    pub(crate) name: &'a str,
    pub(crate) base: Option<&'a str>,
    pub(crate) abstraction: Abstraction,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Member<'a> {
    pub(crate) record: MemberRecord,
    pub(crate) name: &'a str,
    pub(crate) detail: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Declared {
    Header,
    Member(usize),
}

#[derive(Default)]
struct LayoutWriter {
    text: String,
    line: u32,
    line_start: usize,
}

impl LayoutWriter {
    fn push(&mut self, fragment: &str) {
        let base = self.text.len();
        self.text.push_str(fragment);
        for (offset, _) in fragment.match_indices('\n') {
            self.line += 1;
            self.line_start = base + offset + 1;
        }
    }

    fn mark(&self) -> Mark {
        // Invariant: a rendered builtin is well under 4GB
        let to_u32 = |value: usize| u32::try_from(value).expect("builtin source fits in u32");
        Mark {
            byte: to_u32(self.text.len()),
            line: self.line,
            column: to_u32(self.text.len() - self.line_start),
        }
    }

    fn push_spanned(&mut self, fragment: &str) -> Span {
        let start = self.mark();
        self.push(fragment);
        Span {
            start,
            end: self.mark(),
        }
    }

    fn spans_from(&self, start: Mark, selection: Span) -> DeclarationSpans {
        DeclarationSpans {
            range: Span {
                start,
                end: self.mark(),
            },
            selection,
        }
    }
}

pub(crate) fn render_type<'a>(
    header: TypeHeader<'_>,
    members: impl IntoIterator<Item = Member<'a>>,
    mut on_declared: impl FnMut(Declared, DeclarationSpans),
) -> String {
    let mut writer = LayoutWriter::default();
    let start = writer.mark();
    if header.abstraction == Abstraction::Abstract {
        writer.push("abstract ");
    }
    writer.push(match header.record {
        TypeRecord::Struct => "struct ",
        TypeRecord::Enum => "enum ",
        TypeRecord::Native | TypeRecord::Class => "class ",
    });
    let header_selection = writer.push_spanned(header.name);
    if let Some(base) = header.base {
        writer.push(" extends ");
        writer.push(base);
    }
    writer.push(" {");

    let mut has_members = false;
    for (index, member) in members.into_iter().enumerate() {
        has_members = true;
        let spans = render_member(&mut writer, index, member);
        on_declared(Declared::Member(index), spans);
    }

    if has_members {
        writer.push("\n");
    }
    writer.push("}\n");
    on_declared(Declared::Header, writer.spans_from(start, header_selection));
    writer.text
}

fn render_member(writer: &mut LayoutWriter, index: usize, member: Member<'_>) -> DeclarationSpans {
    if member.record == MemberRecord::EnumOption && index > 0 {
        writer.push(",");
    }
    writer.push("\n    ");
    let start = writer.mark();
    let selection = match member.record {
        MemberRecord::Field => {
            writer.push("import var ");
            let selection = writer.push_spanned(member.name);
            writer.push(" : ");
            writer.push(member.detail);
            writer.push(";");
            selection
        }
        MemberRecord::Method => {
            writer.push("import function ");
            let selection = writer.push_spanned(member.name);
            writer.push("(...);");
            selection
        }
        MemberRecord::EnumOption => {
            let selection = writer.push_spanned(member.name);
            writer.push(" = ");
            writer.push(member.detail);
            selection
        }
    };
    writer.spans_from(start, selection)
}

pub(crate) fn render_globals<'a>(
    names: impl IntoIterator<Item = &'a str>,
    mut on_declared: impl FnMut(usize, DeclarationSpans),
) -> String {
    let mut writer = LayoutWriter::default();
    for (index, name) in names.into_iter().enumerate() {
        let start = writer.mark();
        writer.push("import function ");
        let selection = writer.push_spanned(name);
        writer.push("(...);");
        on_declared(index, writer.spans_from(start, selection));
        writer.push("\n");
    }
    writer.text
}
