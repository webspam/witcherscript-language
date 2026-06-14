use lsp_types::{
    Command, CompletionItem, CompletionItemKind, CompletionTextEdit, InsertTextFormat,
    ParameterInformation, ParameterLabel, Range, SignatureHelp, SignatureInformation, TextEdit,
    Url,
};
use witcherscript_language::formatter::ColonSpacing;
use witcherscript_language::resolve::{
    Definition, SignatureHelpInfo, SymbolDb, render_parameters, render_signature,
};
use witcherscript_language::symbols::{Symbol, SymbolKind};
use witcherscript_language::types::Type;

// Identifies the symbol behind a completion item so resolve can re-derive its documentation.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct CompletionItemData {
    /// Document where completion was invoked; routes loose vs workspace at resolve time.
    pub(crate) origin: Url,
    pub(crate) def_uri: String,
    pub(crate) selection: std::ops::Range<usize>,
    pub(crate) name: String,
    /// Owning container; a generic instance (`array<int>`) drives re-substitution at resolve.
    #[serde(default)]
    pub(crate) container: Option<String>,
}

pub(crate) fn completion_item(
    definition: &Definition,
    db: &SymbolDb,
    origin: &Url,
) -> CompletionItem {
    let symbol = &definition.symbol;
    let is_callable = symbol.kind.is_callable();
    let kind = Some(match symbol.kind {
        SymbolKind::Method | SymbolKind::Event => CompletionItemKind::METHOD,
        SymbolKind::Field => CompletionItemKind::FIELD,
        SymbolKind::Function => CompletionItemKind::FUNCTION,
        SymbolKind::Variable | SymbolKind::Parameter => CompletionItemKind::VARIABLE,
        _ => CompletionItemKind::TEXT,
    });
    let (detail, snippet_params) = if is_callable {
        let params = db.display_parameters_of(definition);
        let detail = render_signature(
            &params,
            symbol.type_annotation.as_ref(),
            ColonSpacing::Compact,
        );
        // Optional parameters stay out of snippet slots (AGENTS.md key invariant #5).
        let snippet_params: Vec<String> = params
            .into_iter()
            .filter(|p| !p.specifiers.is_optional())
            .map(|p| p.name)
            .collect();
        (Some(detail), snippet_params)
    } else {
        let detail = symbol.type_annotation.as_ref().map(ToString::to_string);
        (detail, Vec::new())
    };
    let (insert_text, insert_text_format) = if is_callable {
        if snippet_params.is_empty() {
            (
                Some(format!("{}()", symbol.name)),
                Some(InsertTextFormat::SNIPPET),
            )
        } else {
            let args = snippet_params
                .iter()
                .enumerate()
                .map(|(i, name)| format!("${{{}:{}}}", i + 1, name))
                .collect::<Vec<_>>()
                .join(", ");
            (
                Some(format!("{}({})$0", symbol.name, args)),
                Some(InsertTextFormat::SNIPPET),
            )
        }
    } else {
        (None, None)
    };
    let command = (is_callable && !snippet_params.is_empty()).then(|| Command {
        title: "Trigger parameter hints".to_string(),
        command: "editor.action.triggerParameterHints".to_string(),
        arguments: None,
    });
    let data = CompletionItemData {
        origin: origin.clone(),
        def_uri: definition.uri.clone(),
        selection: symbol.selection_byte_range.clone(),
        name: symbol.name.clone(),
        container: symbol.container_name.clone(),
    };
    CompletionItem {
        label: symbol.name.clone(),
        kind,
        detail,
        insert_text,
        insert_text_format,
        command,
        data: Some(serde_json::to_value(data).expect("CompletionItemData always serializes")),
        ..CompletionItem::default()
    }
}

pub(crate) fn signature_help_response(info: SignatureHelpInfo) -> SignatureHelp {
    let parameters = info
        .parameters
        .into_iter()
        .map(|(start, end)| ParameterInformation {
            label: ParameterLabel::LabelOffsets([wire_u32(start), wire_u32(end)]),
            documentation: None,
        })
        .collect();
    SignatureHelp {
        signatures: vec![SignatureInformation {
            label: info.label,
            documentation: None,
            parameters: Some(parameters),
            active_parameter: None,
        }],
        active_signature: Some(0),
        active_parameter: info.active_parameter.map(wire_u32),
    }
}

/// Saturates: LSP wire offsets are u32; values past that clamp to `u32::MAX`.
fn wire_u32(n: usize) -> u32 {
    n.try_into().unwrap_or(u32::MAX)
}

fn method_signature(name: &str, params: &[Symbol]) -> String {
    format!("{name}{}", render_parameters(params, ColonSpacing::Spaced))
}

pub(crate) fn wrap_method_snippet(method: &Definition, db: &SymbolDb) -> String {
    let params = db.full_parameters_of(&method.uri, method.symbol.id);
    let signature = method_signature(&method.symbol.name, &params);
    let call_args = params
        .iter()
        .map(|p| p.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let has_return = method.symbol.kind == SymbolKind::Event
        || method
            .symbol
            .type_annotation
            .as_ref()
            .is_some_and(|t| *t != Type::Void);
    let body = if has_return {
        format!("{{\n\t$0\n\n\treturn wrappedMethod({call_args});\n}}")
    } else {
        format!("{{\n\twrappedMethod({call_args});\n\n\t$0\n}}")
    };
    format!("{signature} {body}")
}

pub(crate) fn replace_method_snippet(method: &Definition, db: &SymbolDb) -> String {
    let params = db.full_parameters_of(&method.uri, method.symbol.id);
    format!(
        "{} {{\n\t$0\n}}",
        method_signature(&method.symbol.name, &params)
    )
}

pub(crate) fn type_completion_item(definition: &Definition) -> CompletionItem {
    let symbol = &definition.symbol;
    let kind = Some(match symbol.kind {
        SymbolKind::Struct => CompletionItemKind::STRUCT,
        SymbolKind::Enum => CompletionItemKind::ENUM,
        _ => CompletionItemKind::CLASS,
    });
    CompletionItem {
        label: symbol.name.clone(),
        kind,
        detail: symbol.display_detail(),
        ..CompletionItem::default()
    }
}

pub(crate) fn builtin_type_item(name: &str) -> CompletionItem {
    CompletionItem {
        label: name.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..CompletionItem::default()
    }
}

pub(crate) fn this_super_item(name: &str) -> CompletionItem {
    CompletionItem {
        label: name.to_string(),
        kind: Some(CompletionItemKind::VARIABLE),
        sort_text: Some(format!("0_{name}")),
        ..CompletionItem::default()
    }
}

pub(crate) fn class_body_kw_item(keyword: &str) -> CompletionItem {
    let (snippet, sort_prefix): (Option<&str>, &str) = match keyword {
        "var" => (Some("var ${1:varName} : ${2:int};"), "0"),
        "function" => (Some("function ${1:Name}($2) {\n\t$0\n}"), "0"),
        "event" => (Some("event On${1}($2) {\n\t$0\n}"), "0"),
        "autobind" => (Some("autobind ${1:name} : ${2:Type} = single;"), "0"),
        "default" => (Some("default ${1:field} = ${2:value};"), "0"),
        "defaults" => (Some("defaults {\n\t${1:field} = ${2:value};\n}"), "0"),
        "hint" => (Some("hint ${1:field} = \"${2:tooltip}\";"), "0"),
        _ => (None, "1"),
    };
    CompletionItem {
        label: keyword.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        insert_text: snippet.map(str::to_string),
        insert_text_format: snippet.map(|_| InsertTextFormat::SNIPPET),
        sort_text: Some(format!("{sort_prefix}_{keyword}")),
        ..CompletionItem::default()
    }
}

pub(crate) fn script_body_item(keyword: &str) -> CompletionItem {
    let (snippet, sort_prefix): (Option<&str>, &str) = match keyword {
        "class" => (Some("class ${1:Name} {\n\t$0\n}"), "0"),
        "state" => (Some("state ${1:Name} in ${2:OwnerClass} {\n\t$0\n}"), "0"),
        "struct" => (Some("struct ${1:Name} {\n\t$0\n}"), "0"),
        "enum" => (Some("enum ${1:Name} {\n\t$0\n}"), "0"),
        "function" => (Some("function ${1:Name}($2) {\n\t$0\n}"), "0"),
        "var" => (Some("var ${1:name} : ${2:Type};"), "0"),
        "@addField" => (Some("@addField(${1:ClassName})"), "0"),
        "@addMethod" => (Some("@addMethod(${1:ClassName})"), "0"),
        "@wrapMethod" => (Some("@wrapMethod(${1:ClassName})"), "0"),
        "@replaceMethod" => (Some("@replaceMethod(${1:ClassName})"), "0"),
        _ => (None, "1"),
    };
    let filter_text = keyword.strip_prefix('@').map(str::to_string);
    CompletionItem {
        label: keyword.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        insert_text: snippet.map(str::to_string),
        insert_text_format: snippet.map(|_| InsertTextFormat::SNIPPET),
        sort_text: Some(format!("{sort_prefix}_{keyword}")),
        filter_text,
        ..CompletionItem::default()
    }
}

pub(crate) fn keyword_snippet_item(label: &str, snippet: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        insert_text: Some(snippet.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        sort_text: Some(format!("0_{label}")),
        ..CompletionItem::default()
    }
}

pub(crate) fn annotation_name_items(replace_range: Range) -> Vec<CompletionItem> {
    [
        // Empty tabstop, not `${1:ClassName}`: the cursor lands in empty parens so the
        // re-triggered suggest opens an unfiltered class list instead of filtering on a placeholder word.
        ("@wrapMethod", "@wrapMethod($1)"),
        ("@addMethod", "@addMethod($1)"),
        ("@replaceMethod", "@replaceMethod($1)"),
        ("@addField", "@addField($1)"),
    ]
    .iter()
    .map(|(label, snippet)| CompletionItem {
        label: label.to_string(),
        filter_text: Some(label.to_string()),
        kind: Some(CompletionItemKind::KEYWORD),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
            range: replace_range,
            new_text: snippet.to_string(),
        })),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        sort_text: Some(format!("0_{label}")),
        // Cursor lands on the class-name placeholder; reopen suggestions there.
        command: Some(Command {
            title: "Suggest class name".to_string(),
            command: "editor.action.triggerSuggest".to_string(),
            arguments: None,
        }),
        ..CompletionItem::default()
    })
    .collect()
}
