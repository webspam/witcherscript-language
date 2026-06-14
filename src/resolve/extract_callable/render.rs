use std::ops::Range;

use crate::document::ParsedDocument;
use crate::formatter::{FormatOptions, indent_block, line_indent};
use crate::symbols::{AccessLevel, Symbol};
use crate::types::Type;

use super::super::extract_common::{Splice, apply_splices};
use super::super::inference::TypeContext;
use super::super::symbol_db::SymbolDb;
use super::captures::{
    BodyRewrite, CapturedLocal, Captures, PromotedField, Receiver, suffixed_unique,
};

pub(super) struct Param {
    name: String,
    ty: Type,
    is_out: bool,
}

pub(super) struct FunctionPlan {
    pub(super) name: String,
    /// Access keyword to prefix (a method renders `private`); a global function has none.
    pub(super) modifier: Option<&'static str>,
    pub(super) receiver: Option<Receiver>,
    pub(super) params: Vec<Param>,
    pub(super) return_type: Type,
    pub(super) body: String,
}

pub(super) fn assemble_params(
    value_locals: &[&CapturedLocal],
    out_locals: &[&CapturedLocal],
    promoted: &[PromotedField],
) -> Vec<Param> {
    let mut params = Vec::new();
    params.extend(value_locals.iter().map(|l| Param {
        name: l.name.clone(),
        ty: l.ty.clone(),
        is_out: false,
    }));
    params.extend(promoted.iter().filter(|p| !p.is_written).map(|p| Param {
        name: p.name.clone(),
        ty: p.ty.clone(),
        is_out: false,
    }));
    params.extend(out_locals.iter().map(|l| Param {
        name: l.name.clone(),
        ty: l.ty.clone(),
        is_out: true,
    }));
    params.extend(promoted.iter().filter(|p| p.is_written).map(|p| Param {
        name: p.name.clone(),
        ty: p.ty.clone(),
        is_out: true,
    }));
    params
}

pub(super) fn moved_text(source: &str, range: &Range<usize>, captures: &Captures) -> String {
    if captures.rewrites.is_empty() {
        return source[range.clone()].to_string();
    }
    let receiver = captures.receiver.as_ref().map(|r| r.param_name.as_str());
    let rel = |r: &Range<usize>| (r.start - range.start)..(r.end - range.start);
    let splices: Vec<Splice> = captures
        .rewrites
        .iter()
        .map(|rewrite| match rewrite {
            BodyRewrite::Qualify(at) => Splice {
                range: at - range.start..at - range.start,
                text: format!("{}.", receiver.expect("qualify implies a receiver")),
            },
            BodyRewrite::ReplaceThis(this) => Splice {
                range: rel(this),
                text: receiver
                    .expect("this-replacement implies a receiver")
                    .to_string(),
            },
            BodyRewrite::ReplacePromoted { range: r, field } => Splice {
                range: rel(r),
                text: captures.promoted[*field].name.clone(),
            },
        })
        .collect();
    apply_splices(&source[range.clone()], &splices)
}

pub(super) fn statement_body(
    source: &str,
    range: &Range<usize>,
    captures: &Captures,
    returned: Option<&CapturedLocal>,
    options: FormatOptions,
) -> String {
    let moved = moved_text(source, range, captures);
    let base = line_indent(source, range.start);
    let mut lines: Vec<String> = Vec::new();
    if let Some(r) = returned {
        lines.push(format!("var {}{}{};", r.name, colon_for(options), r.ty));
    }
    for (i, line) in moved.lines().enumerate() {
        match i {
            0 => lines.push(line.to_string()),
            _ => lines.push(dedent_line(line, base).to_string()),
        }
    }
    if let Some(r) = returned {
        lines.push(format!("return {};", r.name));
    }
    lines.join("\n")
}

fn dedent_line<'a>(line: &'a str, base: &str) -> &'a str {
    if let Some(stripped) = line.strip_prefix(base) {
        return stripped;
    }
    // Mixed tabs/spaces: drop at most the base's width of leading whitespace.
    let mut rest = line;
    for _ in 0..base.len() {
        match rest.strip_prefix([' ', '\t']) {
            Some(stripped) => rest = stripped,
            None => break,
        }
    }
    rest
}

pub(super) fn call_expression(plan: &FunctionPlan) -> String {
    let mut args = Vec::new();
    if plan.receiver.is_some() {
        args.push("this");
    }
    args.extend(plan.params.iter().map(|p| p.name.as_str()));
    format!("{}({})", plan.name, args.join(", "))
}

pub(super) fn render_function(plan: &FunctionPlan, options: FormatOptions) -> String {
    let colon = colon_for(options);
    let mut params = Vec::new();
    if let Some(receiver) = &plan.receiver {
        params.push(format!(
            "{}{colon}{}",
            receiver.param_name, receiver.type_name
        ));
    }
    params.extend(plan.params.iter().map(|p| {
        let out = if p.is_out { "out " } else { "" };
        format!("{out}{}{colon}{}", p.name, p.ty)
    }));
    let params = params.join(", ");
    let return_clause = match &plan.return_type {
        Type::Void => String::new(),
        ty => format!("{colon}{ty}"),
    };
    let body = indent_block(&plan.body, &options);
    let prefix = plan.modifier.map_or(String::new(), |m| format!("{m} "));
    format!(
        "{prefix}function {}({params}){return_clause} {{\n{body}\n}}",
        plan.name
    )
}

fn colon_for(options: FormatOptions) -> &'static str {
    if options.compact_colon { ": " } else { " : " }
}

pub(super) fn unique_function_name(
    document: &ParsedDocument,
    db: &SymbolDb,
    callable: &Symbol,
    type_context: Option<&TypeContext>,
    base: &str,
) -> String {
    // A clash with anything the call-site lookup reaches first would bind the call elsewhere.
    let taken = |name: &str| {
        document
            .symbols
            .children_of(Some(callable.id))
            .any(|s| s.name == name)
            || document.symbols.top_level_by_name(name).is_some()
            || db.find_top_level(name).is_some()
            || db.find_script_global(name).is_some()
            || type_context.is_some_and(|ctx| {
                db.find_member(&ctx.name, name, AccessLevel::Private)
                    .is_some()
            })
    };
    suffixed_unique(base, taken)
}
