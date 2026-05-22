use super::super::{resolve_definition, SymbolDb, WorkspaceIndex};
use super::{make_doc, make_index};
use crate::builtins::load_builtins_index;
use crate::line_index::SourcePosition;
use crate::symbols::SymbolKind;

const CR4_HUD_MODULE_URI: &str = "witcherscript-builtin:/CR4HudModule.ws";

fn builtins_db<'a>(
    workspace: &'a WorkspaceIndex,
    base: &'a WorkspaceIndex,
    builtins: &'a WorkspaceIndex,
) -> SymbolDb<'a> {
    SymbolDb::new(workspace, base).with_builtins(builtins)
}

#[test]
fn builtin_class_is_indexed_as_a_global_type() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    let def = db
        .find_top_level("CR4HudModule")
        .expect("CR4HudModule should be indexed");
    assert_eq!(def.symbol.kind, SymbolKind::Class);
    assert_eq!(def.uri, CR4_HUD_MODULE_URI);
    assert_eq!(def.symbol.base_class.as_deref(), Some("CHudModule"));
}

#[test]
fn builtin_class_appears_in_type_completions() {
    let builtins = load_builtins_index();
    let empty = WorkspaceIndex::default();
    let db = builtins_db(&empty, &empty, &builtins);

    let types = db.all_types();
    assert!(
        types.iter().any(|d| d.symbol.name == "CR4HudModule"),
        "builtin class should appear in all_types(); got {:?}",
        types.iter().map(|d| &d.symbol.name).collect::<Vec<_>>()
    );
    assert!(
        !types.iter().any(|d| d.symbol.name == "array"),
        "the generic array builtin must still be excluded from all_types()"
    );
}

#[test]
fn goto_definition_on_builtin_class_resolves_into_builtin_file() {
    let source = "function Test() {\n  var m : CR4HudModule;\n}\n";
    let doc = make_doc(source);
    let workspace = make_index("file:///test.ws", &doc);
    let empty = WorkspaceIndex::default();
    let builtins = load_builtins_index();
    let db = builtins_db(&workspace, &empty, &builtins);

    let def = resolve_definition(
        "file:///test.ws",
        &doc,
        &db,
        SourcePosition {
            line: 1,
            character: 12,
        },
    )
    .expect("CR4HudModule should resolve");

    assert_eq!(def.uri, CR4_HUD_MODULE_URI);
    assert_eq!(def.symbol.name, "CR4HudModule");
}
