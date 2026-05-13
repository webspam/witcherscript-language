use super::super::resolve_definition;
use super::{make_doc, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;

#[test]
fn resolves_method_on_function_return_value() {
    let source = concat!(
        "class Duck {\n",
        "  public function Quack() {}\n",
        "}\n",
        "function ReturnsDuck() : Duck {}\n",
        "function Test() {\n",
        "  ReturnsDuck().Quack();\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    // cursor on 'Quack' — line 5, col 16
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 5,
            character: 16,
        },
    )
    .expect("Quack should resolve via return type of ReturnsDuck");

    assert_eq!(definition.symbol.name, "Quack");
    let container_id = definition.symbol.container.expect("method has a container");
    let container = doc.symbols.by_id(container_id).expect("container exists");
    assert_eq!(container.name, "Duck");
}

#[test]
fn resolves_chained_call_method() {
    // ReturnsDuck().GetNest().Count() — two levels of chaining
    let source = concat!(
        "class Nest {\n",
        "  public function Count() : int {}\n",
        "}\n",
        "class Duck {\n",
        "  public function GetNest() : Nest {}\n",
        "}\n",
        "function ReturnsDuck() : Duck {}\n",
        "function Test() {\n",
        "  ReturnsDuck().GetNest().Count();\n",
        "}\n",
    );
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    // cursor on 'Count' — line 8, col 26
    let definition = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 8,
            character: 26,
        },
    )
    .expect("Count should resolve via chained return types");

    assert_eq!(definition.symbol.name, "Count");
    let container_id = definition.symbol.container.expect("method has a container");
    let container = doc.symbols.by_id(container_id).expect("container exists");
    assert_eq!(container.name, "Nest");
}
