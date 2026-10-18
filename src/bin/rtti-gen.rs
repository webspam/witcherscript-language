use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use witcherscript_language::types::is_builtin_type_name;

#[path = "../builtins/rtti_tags.rs"]
mod rtti_tags;

const DEFAULT_INPUT: &str = "target/tmp/rttidump.xml";
const DEFAULT_OUTPUT: &str = "builtins/rtti.tsv";
const OBJECT_ROOT: &str = "IReferencable";

#[derive(Parser)]
#[command(about = "Compiles an RTTI XML dump into the compact builtins table")]
struct Args {
    #[arg(default_value = DEFAULT_INPUT)]
    input: PathBuf,
    #[arg(default_value = DEFAULT_OUTPUT)]
    output: PathBuf,
}

struct Class {
    name: String,
    base: Option<String>,
    is_abstract: bool,
    fields: Vec<(String, String)>,
    methods: Vec<String>,
}

struct Enum {
    name: String,
    options: Vec<(String, String)>,
}

#[derive(Default)]
struct Layout {
    classes: Vec<Class>,
    enums: Vec<Enum>,
    simples: Vec<String>,
    globals: Vec<String>,
}

fn attr<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!(" {key}=\"");
    let start = line.find(&needle)? + needle.len();
    let end = line[start..].find('"')? + start;
    Some(&line[start..end])
}

fn required_attr<'a>(line: &'a str, key: &str, line_no: usize) -> Result<&'a str> {
    attr(line, key).with_context(|| format!("line {line_no}: missing {key}"))
}

fn normalise_type(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix('[') {
        let element = rest.split_once(']').map_or(rest, |(_, e)| e);
        return format!("array<{}>", normalise_type(element));
    }
    for prefix in ["array:", "static:"] {
        if let Some(mut element) = raw.strip_prefix(prefix) {
            while let Some((head, tail)) = element.split_once(',')
                && head.chars().all(|c| c.is_ascii_digit())
            {
                element = tail;
            }
            return format!("array<{}>", normalise_type(element));
        }
    }
    for prefix in ["ptr:", "handle:", "soft:"] {
        if let Some(rest) = raw.strip_prefix(prefix) {
            return normalise_type(rest);
        }
    }
    raw.to_string()
}

fn leaf_type(normalised: &str) -> &str {
    let mut current = normalised;
    while let Some(inner) = current
        .strip_prefix("array<")
        .and_then(|s| s.strip_suffix('>'))
    {
        current = inner;
    }
    current
}

fn parse_layout(xml: &str) -> Result<Layout> {
    let mut layout = Layout::default();
    let mut current_class: Option<Class> = None;
    let mut current_enum: Option<Enum> = None;
    for (index, raw_line) in xml.lines().enumerate() {
        let line_no = index + 1;
        let line = raw_line.trim();
        if line.starts_with("<class ") {
            let class = Class {
                name: required_attr(line, "Name", line_no)?.to_string(),
                base: attr(line, "BaseClass").map(str::to_string),
                is_abstract: attr(line, "IsAbstract") == Some("true"),
                fields: Vec::new(),
                methods: Vec::new(),
            };
            if line.ends_with("/>") {
                layout.classes.push(class);
            } else {
                current_class = Some(class);
            }
        } else if line.starts_with("</class>") {
            layout
                .classes
                .push(current_class.take().context("unbalanced </class>")?);
        } else if line.starts_with("<property ") {
            let class = current_class
                .as_mut()
                .with_context(|| format!("line {line_no}: property outside class"))?;
            class.fields.push((
                required_attr(line, "Name", line_no)?.to_string(),
                normalise_type(required_attr(line, "Type", line_no)?),
            ));
        } else if line.starts_with("<function ") {
            let class = current_class
                .as_mut()
                .with_context(|| format!("line {line_no}: function outside class"))?;
            class
                .methods
                .push(required_attr(line, "Name", line_no)?.to_string());
        } else if line.starts_with("<globalFunction ") {
            layout
                .globals
                .push(required_attr(line, "Name", line_no)?.to_string());
        } else if line.starts_with("<enum ") {
            current_enum = Some(Enum {
                name: required_attr(line, "Name", line_no)?.to_string(),
                options: Vec::new(),
            });
        } else if line.starts_with("<option ") {
            let enumeration = current_enum
                .as_mut()
                .with_context(|| format!("line {line_no}: option outside enum"))?;
            enumeration.options.push((
                required_attr(line, "Name", line_no)?.to_string(),
                required_attr(line, "Value", line_no)?.to_string(),
            ));
        } else if line.starts_with("</enum>") {
            layout
                .enums
                .push(current_enum.take().context("unbalanced </enum>")?);
        } else if line.starts_with("<simple ") {
            layout
                .simples
                .push(required_attr(line, "Name", line_no)?.to_string());
        }
    }
    if current_class.is_some() || current_enum.is_some() {
        bail!("unterminated class or enum at end of dump");
    }
    Ok(layout)
}

fn is_object_class(name: &str, bases: &HashMap<&str, Option<&str>>) -> Result<bool> {
    let mut seen = HashSet::new();
    let mut current = Some(name);
    while let Some(candidate) = current {
        if candidate == OBJECT_ROOT {
            return Ok(true);
        }
        if !seen.insert(candidate) {
            bail!("class hierarchy cycle through {candidate} while resolving {name}");
        }
        current = bases.get(candidate).copied().flatten();
    }
    Ok(false)
}

fn undeclared_enum_types(layout: &Layout) -> BTreeSet<String> {
    let declared: HashSet<&str> = layout
        .classes
        .iter()
        .map(|c| c.name.as_str())
        .chain(layout.enums.iter().map(|e| e.name.as_str()))
        .chain(layout.simples.iter().map(String::as_str))
        .collect();
    layout
        .classes
        .iter()
        .flat_map(|c| c.fields.iter().map(|(_, ty)| leaf_type(ty)))
        .filter(|leaf| {
            !declared.contains(leaf) && !is_builtin_type_name(leaf) && leaf.starts_with('E')
        })
        .map(str::to_string)
        .collect()
}

fn render(layout: &Layout) -> Result<String> {
    let bases: HashMap<&str, Option<&str>> = layout
        .classes
        .iter()
        .map(|c| (c.name.as_str(), c.base.as_deref()))
        .collect();
    let mut out = String::new();
    for simple in layout.simples.iter().filter(|s| !is_builtin_type_name(s)) {
        let _ = writeln!(out, "{}\t{simple}", rtti_tags::NATIVE);
    }
    for class in &layout.classes {
        let tag = if is_object_class(&class.name, &bases)? {
            rtti_tags::CLASS
        } else {
            rtti_tags::STRUCT
        };
        let flags = if class.is_abstract {
            rtti_tags::ABSTRACT_FLAG
        } else {
            ""
        };
        let base = class.base.as_deref().unwrap_or("");
        let _ = writeln!(out, "{tag}\t{}\t{base}\t{flags}", class.name);
        for (name, ty) in &class.fields {
            let _ = writeln!(out, "{}\t{name}\t{ty}", rtti_tags::FIELD);
        }
        for name in &class.methods {
            let _ = writeln!(out, "{}\t{name}", rtti_tags::METHOD);
        }
    }
    for enumeration in &layout.enums {
        let _ = writeln!(out, "{}\t{}", rtti_tags::ENUM, enumeration.name);
        for (name, value) in &enumeration.options {
            let _ = writeln!(out, "{}\t{name}\t{value}", rtti_tags::ENUM_OPTION);
        }
    }
    for name in undeclared_enum_types(layout) {
        let _ = writeln!(out, "{}\t{name}", rtti_tags::ENUM);
    }
    for name in &layout.globals {
        let _ = writeln!(out, "{}\t{name}", rtti_tags::GLOBAL_FUNCTION);
    }
    Ok(out)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let xml = std::fs::read_to_string(&args.input)
        .with_context(|| format!("reading {}", args.input.display()))?;
    let layout = parse_layout(&xml)?;
    let table = render(&layout)?;
    std::fs::write(&args.output, &table)
        .with_context(|| format!("writing {}", args.output.display()))
}
