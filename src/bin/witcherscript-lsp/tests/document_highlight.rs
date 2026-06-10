use lsp_types::DocumentHighlightKind;
use witcherscript_language::resolve::document_highlights;
use witcherscript_language::test_support::TestDb;

use crate::convert::document_highlight;

#[test]
fn converts_resolve_kinds_to_lsp_document_highlights() {
    let t = TestDb::new("function F() {\n var x : int;\n $0x = 1;\n Use(x);\n}\n");
    let (uri, pos) = t.cursor();
    let hits = document_highlights(&uri, t.doc_for(&uri), &t.db(), pos).expect("symbol resolves");

    let lsp: Vec<_> = hits
        .into_iter()
        .map(|(range, kind)| document_highlight(range, kind))
        .collect();

    let kinds: Vec<_> = lsp.iter().map(|h| h.kind).collect();
    assert_eq!(
        kinds,
        vec![
            Some(DocumentHighlightKind::WRITE),
            Some(DocumentHighlightKind::WRITE),
            Some(DocumentHighlightKind::READ),
        ],
        "declaration and assignment are WRITE, the read use is READ"
    );

    let decl = &lsp[0];
    assert_eq!(decl.range.start.line, 1, "declaration is on line 1");
    assert_eq!(
        decl.range.start.character, 5,
        "declaration name starts at col 5"
    );
}
