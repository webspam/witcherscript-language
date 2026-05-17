use std::collections::HashMap;

use tracing::{debug, trace};
use tree_sitter::Node;

use crate::document::ParsedDocument;
use crate::resolve::{infer_expr_type_memo, resolve_definition_at_byte, SymbolDb, BUILTIN_TYPES};
use crate::symbols::{AccessLevel, SymbolKind};

use super::{run_rules_on_document, CstRule, CstRuleCtx, Severity, WorkspaceDiagnostic};

pub(crate) struct UnknownSymbolRule;

impl CstRule for UnknownSymbolRule {
    fn interested_in(&self, kind: &str) -> bool {
        kind == "ident"
    }

    fn visit<'tree>(&self, node: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) {
        check_ident(node, ctx);
    }
}

pub fn collect_unknown_symbol_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let rule = UnknownSymbolRule;
    let rules: Vec<&dyn CstRule> = vec![&rule];
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, document) in documents {
        let diagnostics = run_rules_on_document(uri, document, db, &rules);
        if !diagnostics.is_empty() {
            debug!(
                uri = %uri,
                count = diagnostics.len(),
                "emitted unknown-symbol diagnostics"
            );
            result.insert((*uri).to_string(), diagnostics);
        }
    }

    trace!(
        documents = documents.len(),
        flagged_uris = result.len(),
        "scanned for unknown symbols"
    );

    result
}

#[derive(Debug)]
enum IdentRole<'tree> {
    Declaration,
    TypeRef,
    MemberOfAccess(Node<'tree>),
    MemberOfDefault,
    FuncBareCall,
    Bare,
}

fn check_ident<'tree>(ident: Node<'tree>, ctx: &mut CstRuleCtx<'_, 'tree>) -> Option<()> {
    if has_error_or_incomplete_ancestor(ident) {
        return None;
    }

    let role = classify(ident)?;

    let name = ident.utf8_text(ctx.document.source.as_bytes()).ok()?;

    if name == "wrappedMethod" && is_inside_wrap_method(ident, ctx) {
        return None;
    }

    match role {
        IdentRole::Declaration => None,
        IdentRole::TypeRef => {
            if BUILTIN_TYPES.contains(&name) {
                return None;
            }
            if resolve_definition_at_byte(ctx.uri, ctx.document, ctx.db, ident.start_byte())
                .is_some()
            {
                return None;
            }
            push(ctx, ident, "unknown_type", format!("unknown type '{name}'"));
            Some(())
        }
        IdentRole::MemberOfAccess(receiver) => {
            let receiver_type = infer_expr_type_memo(
                ctx.uri,
                ctx.document,
                ctx.db,
                receiver,
                ident.start_byte(),
                ctx.type_memo,
            )?;
            let top = ctx.db.find_top_level(&receiver_type)?;
            if !matches!(
                top.symbol.kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::State
            ) {
                return None;
            }
            if ctx
                .db
                .find_member(&receiver_type, name, AccessLevel::Private)
                .is_some()
            {
                return None;
            }
            push(
                ctx,
                ident,
                "unknown_member",
                format!("no member '{name}' on type '{receiver_type}'"),
            );
            Some(())
        }
        IdentRole::MemberOfDefault => {
            let enclosing = ctx.document.symbols.enclosing_symbol_at(
                ident.start_byte(),
                &[SymbolKind::Class, SymbolKind::Struct, SymbolKind::State],
            )?;
            let container_name = enclosing.name.clone();
            if ctx
                .db
                .find_member(&container_name, name, AccessLevel::Private)
                .is_some()
            {
                return None;
            }
            push(
                ctx,
                ident,
                "unknown_member",
                format!("no member '{name}' on type '{container_name}'"),
            );
            Some(())
        }
        IdentRole::FuncBareCall => {
            if resolve_definition_at_byte(ctx.uri, ctx.document, ctx.db, ident.start_byte())
                .is_some()
            {
                return None;
            }
            push(
                ctx,
                ident,
                "unknown_function",
                format!("unknown function '{name}'"),
            );
            Some(())
        }
        IdentRole::Bare => {
            if resolve_definition_at_byte(ctx.uri, ctx.document, ctx.db, ident.start_byte())
                .is_some()
            {
                return None;
            }
            push(
                ctx,
                ident,
                "unknown_identifier",
                format!("unknown identifier '{name}'"),
            );
            Some(())
        }
    }
}

fn classify(ident: Node<'_>) -> Option<IdentRole<'_>> {
    let parent = ident.parent()?;

    if is_declaration(ident, parent) {
        return Some(IdentRole::Declaration);
    }

    if is_type_reference(ident, parent) {
        return Some(IdentRole::TypeRef);
    }

    if matches!(
        parent.kind(),
        "member_default_val" | "member_default_val_block_assign" | "member_hint"
    ) && parent.child_by_field_name("member").map(|n| n.id()) == Some(ident.id())
    {
        return Some(IdentRole::MemberOfDefault);
    }

    if parent.kind() == "member_access_expr" {
        let is_member = parent.child_by_field_name("member").map(|n| n.id()) == Some(ident.id());
        if is_member {
            if let Some(grandparent) = parent.parent() {
                if grandparent.kind() == "func_call_expr"
                    && grandparent.child_by_field_name("func").map(|n| n.id()) == Some(parent.id())
                {
                    return None;
                }
            }
            let receiver = parent.child_by_field_name("accessor")?;
            return Some(IdentRole::MemberOfAccess(receiver));
        }
    }

    if parent.kind() == "func_call_expr"
        && parent.child_by_field_name("func").map(|n| n.id()) == Some(ident.id())
    {
        return Some(IdentRole::FuncBareCall);
    }

    Some(IdentRole::Bare)
}

fn is_declaration(ident: Node, parent: Node) -> bool {
    match parent.kind() {
        "class_decl" | "struct_decl" | "enum_decl" | "state_decl" | "func_decl" | "event_decl"
        | "autobind_decl" | "enum_decl_variant" => {
            parent.child_by_field_name("name").map(|n| n.id()) == Some(ident.id())
        }
        "func_param_group" | "local_var_decl_stmt" | "member_var_decl" => {
            let mut cursor = parent.walk();
            let found = parent
                .children_by_field_name("names", &mut cursor)
                .any(|n| n.id() == ident.id());
            found
        }
        _ => false,
    }
}

fn is_type_reference(ident: Node, parent: Node) -> bool {
    match parent.kind() {
        "class_decl" | "state_decl" => {
            parent.child_by_field_name("base").map(|n| n.id()) == Some(ident.id())
                || parent.child_by_field_name("parent").map(|n| n.id()) == Some(ident.id())
        }
        "type_annot" => parent.child_by_field_name("type_name").map(|n| n.id()) == Some(ident.id()),
        "new_expr" => parent.child_by_field_name("class").map(|n| n.id()) == Some(ident.id()),
        "annotation" => parent.child_by_field_name("arg").map(|n| n.id()) == Some(ident.id()),
        "cast_expr" => {
            let mut cursor = parent.walk();
            let found = parent
                .children_by_field_name("type", &mut cursor)
                .any(|n| n.id() == ident.id());
            found
        }
        _ => false,
    }
}

fn is_inside_wrap_method<'tree>(ident: Node<'tree>, ctx: &CstRuleCtx<'_, 'tree>) -> bool {
    let Some(enclosing) = ctx.document.symbols.enclosing_symbol_at(
        ident.start_byte(),
        &[SymbolKind::Function, SymbolKind::Method],
    ) else {
        return false;
    };
    enclosing.annotations.iter().any(|a| a.name == "wrapMethod")
}

fn has_error_or_incomplete_ancestor(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.is_error() || parent.is_missing() {
            return true;
        }
        if parent.kind() == "incomplete_member_access_expr" {
            return true;
        }
        current = parent;
    }
    false
}

fn push<'tree>(ctx: &mut CstRuleCtx<'_, 'tree>, ident: Node<'tree>, kind: &str, message: String) {
    let range = ctx.document.line_index.byte_range_to_range(
        &ctx.document.source,
        ident.start_byte(),
        ident.end_byte(),
    );
    ctx.diagnostics.push(WorkspaceDiagnostic {
        kind: kind.to_string(),
        message,
        severity: Severity::Error,
        range,
        related: vec![],
    });
}

#[cfg(test)]
mod tests {
    use super::collect_unknown_symbol_diagnostics;
    use crate::document::{parse_document, ParsedDocument};
    use crate::resolve::{SymbolDb, WorkspaceIndex};

    fn index_and_docs(docs: &[(&str, &str)]) -> (WorkspaceIndex, Vec<(String, ParsedDocument)>) {
        let mut idx = WorkspaceIndex::default();
        let mut parsed = Vec::new();
        for (uri, src) in docs {
            let doc = parse_document(*src).expect("parse should succeed");
            idx.update_document(*uri, &doc);
            parsed.push((uri.to_string(), doc));
        }
        (idx, parsed)
    }

    fn check(
        idx: &WorkspaceIndex,
        docs: &[(String, ParsedDocument)],
    ) -> std::collections::HashMap<String, Vec<super::WorkspaceDiagnostic>> {
        let base = WorkspaceIndex::default();
        let db = SymbolDb::new(idx, &base);
        let doc_pairs: Vec<(&str, &ParsedDocument)> =
            docs.iter().map(|(uri, doc)| (uri.as_str(), doc)).collect();
        collect_unknown_symbol_diagnostics(&doc_pairs, &db)
    }

    fn kinds(diags: &[super::WorkspaceDiagnostic]) -> Vec<&str> {
        diags.iter().map(|d| d.kind.as_str()).collect()
    }

    #[test]
    fn declarations_do_not_fire() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Foo {} \
             struct S {} \
             enum E { V } \
             function F(a, b : int) { var x, y : int; } \
             event Ev() {} \
             state St in Foo { entry function Run() {} }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "no diagnostics expected, got {result:?}");
    }

    #[test]
    fn unknown_type_in_extends() {
        let (idx, docs) = index_and_docs(&[("file:///t.ws", "class Foo extends NoSuch {}\n")]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_type"]);
        assert!(diags[0].message.contains("NoSuch"));
    }

    #[test]
    fn unknown_type_in_state_parent() {
        let (idx, docs) = index_and_docs(&[("file:///t.ws", "state Drive in NoSuch { }\n")]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_type"]);
        assert!(diags[0].message.contains("NoSuch"));
    }

    #[test]
    fn unknown_type_in_var_annot() {
        let (idx, docs) = index_and_docs(&[("file:///t.ws", "function F() { var x : NoSuch; }\n")]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_type"]);
        assert!(diags[0].message.contains("NoSuch"));
    }

    #[test]
    fn unknown_type_in_new_expr() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Owner {} function F() { var o : Owner; var x : Owner; x = new NoSuch in o; }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert!(
            kinds(diags).contains(&"unknown_type"),
            "expected unknown_type, got {:?}",
            kinds(diags)
        );
    }

    #[test]
    fn unknown_type_in_annotation_arg() {
        let (idx, docs) =
            index_and_docs(&[("file:///t.ws", "@addMethod(NoSuch) function Extra() {}\n")]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_type"]);
        assert!(diags[0].message.contains("NoSuch"));
    }

    #[test]
    fn unknown_type_in_cast() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A {} function F() { var a : A; var b : A; b = (NoSuch) a; }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_type"]);
        assert!(diags[0].message.contains("NoSuch"));
    }

    #[test]
    fn builtin_types_skipped() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "function F(a : bool, b : int, c : float, d : string, e : name, f : byte) : void {}\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn builtin_type_aliases_skipped() {
        let cases: &[(&str, &str)] = &[
            ("Bool", "function F(a : Bool) {}\n"),
            ("Float", "function F(a : Float) {}\n"),
            ("String", "function F(a : String) {}\n"),
            ("CName", "function F(a : CName) {}\n"),
            ("Int32", "function F(a : Int32) {}\n"),
            ("UInt8", "function F(a : UInt8) {}\n"),
            ("Int16", "function F(a : Int16) {}\n"),
            ("Int8", "function F(a : Int8) {}\n"),
            ("Uint32", "function F(a : Uint32) {}\n"),
            ("Uint16", "function F(a : Uint16) {}\n"),
            ("StringAnsi", "function F(a : StringAnsi) {}\n"),
        ];
        for (label, src) in cases {
            let (idx, docs) = index_and_docs(&[("file:///t.ws", *src)]);
            let result = check(&idx, &docs);
            assert!(
                result.is_empty(),
                "case {label}: expected no diagnostics, got {result:?}",
            );
        }
    }

    #[test]
    fn known_type_skipped() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A {} class B extends A { var a : A; }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn unknown_member_on_known_receiver() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A { var known : int; } function F() { var a : A; a.bogus = 1; }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_member"]);
        assert!(diags[0].message.contains("bogus"));
        assert!(diags[0].message.contains("'A'"));
    }

    #[test]
    fn unknown_member_default_val() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A { var known : int; default bogus = 1; }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_member"]);
        assert!(diags[0].message.contains("bogus"));
    }

    #[test]
    fn unknown_member_default_block() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A { var known : int; defaults { bogus = 1; } }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_member"]);
        assert!(diags[0].message.contains("bogus"));
    }

    #[test]
    fn unknown_member_hint() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A { var known : int; hint bogus = \"tip\"; }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_member"]);
        assert!(diags[0].message.contains("bogus"));
    }

    #[test]
    fn known_member_skipped() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A { var known : int; } function F() { var a : A; a.known = 1; }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn private_member_skipped() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A { private var hidden : int; function R() { var a : A; a.hidden = 1; } }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn cascading_unknown_receiver_skips_member() {
        let (idx, docs) =
            index_and_docs(&[("file:///t.ws", "function F(x : NoSuch) { x.field = 1; }\n")]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        let codes = kinds(diags);
        assert!(
            codes.contains(&"unknown_type"),
            "expected unknown_type for NoSuch, got {codes:?}"
        );
        assert!(
            !codes.contains(&"unknown_member"),
            "should not flag .field when receiver type unknown, got {codes:?}"
        );
    }

    #[test]
    fn primitive_receiver_skipped() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "function F() { var n : int; n.field = 1; }\n",
        )]);
        let result = check(&idx, &docs);
        let codes = result
            .get("file:///t.ws")
            .map(|d| kinds(d))
            .unwrap_or_default();
        assert!(
            !codes.contains(&"unknown_member"),
            "should not flag .field on primitive, got {codes:?}"
        );
    }

    #[test]
    fn unknown_function_bare_call() {
        let (idx, docs) = index_and_docs(&[("file:///t.ws", "function F() { Bogus(); }\n")]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_function"]);
        assert!(diags[0].message.contains("Bogus"));
    }

    #[test]
    fn known_function_skipped() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "function Helper() {} function F() { Helper(); }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn this_shorthand_method_call_skipped() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A { function Helper() {} function Run() { Helper(); } }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn this_shorthand_inherited_method_call_skipped() {
        let (idx, docs) = index_and_docs(&[
            ("file:///a.ws", "class Base { function Helper() {} }\n"),
            (
                "file:///b.ws",
                "class Child extends Base { function Run() { Helper(); } }\n",
            ),
        ]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn unknown_identifier_bare() {
        let (idx, docs) =
            index_and_docs(&[("file:///t.ws", "function F() { var x : int; x = bogus; }\n")]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_identifier"]);
        assert!(diags[0].message.contains("bogus"));
    }

    #[test]
    fn known_local_skipped() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "function F() { var x : int; var y : int; y = x; }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn known_parameter_skipped() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "function F(p : int) { var y : int; y = p; }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn this_shorthand_field_skipped() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A { var known : int; function R() { var y : int; y = known; } }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn method_call_not_duplicated_as_member() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A {} function F() { var a : A; a.Bogus(); }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result
            .get("file:///t.ws")
            .map(|d| kinds(d))
            .unwrap_or_default();
        assert!(
            !diags.contains(&"unknown_member"),
            "should defer method call to unknown_method, got {diags:?}"
        );
    }

    #[test]
    fn parent_state_owner_member_skipped() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "statemachine class Owner { function Help() {} } \
             state St in Owner { entry function Run() { parent.Help(); } }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn array_generic_produces_noise() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class A {} function F() { var xs : array<A>; }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        let codes = kinds(diags);
        assert!(
            codes.contains(&"unknown_type"),
            "expected unknown_type on 'array' (acknowledged noise), got {codes:?}"
        );
    }

    #[test]
    fn no_noise_inside_error_subtree() {
        let (idx, docs) =
            index_and_docs(&[("file:///t.ws", "function F() { x +=== bogus = ; }\n")]);
        let _ = check(&idx, &docs);
    }

    #[test]
    fn wrapped_method_call_inside_wrap_method_not_flagged() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Foo {} \
             @wrapMethod(Foo) function W() { wrappedMethod(); }\n",
        )]);
        let result = check(&idx, &docs);
        assert!(
            result.is_empty(),
            "wrappedMethod inside @wrapMethod should not be flagged, got {result:?}"
        );
    }

    #[test]
    fn wrapped_method_call_outside_wrap_method_still_flagged() {
        let (idx, docs) =
            index_and_docs(&[("file:///t.ws", "function F() { wrappedMethod(); }\n")]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_function"]);
        assert!(diags[0].message.contains("wrappedMethod"));
    }

    #[test]
    fn wrapped_method_in_add_method_still_flagged() {
        let (idx, docs) = index_and_docs(&[(
            "file:///t.ws",
            "class Foo {} \
             @addMethod(Foo) function A() { wrappedMethod(); }\n",
        )]);
        let result = check(&idx, &docs);
        let diags = result.get("file:///t.ws").unwrap();
        assert_eq!(kinds(diags), vec!["unknown_function"]);
        assert!(diags[0].message.contains("wrappedMethod"));
    }
}
