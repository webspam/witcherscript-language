use super::EditorSession;

pub(crate) async fn snapshot_battery(session: &mut EditorSession, fixture: &str) {
    for rel in session.rel_paths() {
        insta::assert_yaml_snapshot!(
            name(fixture, &rel, "diagnostics"),
            session.diagnostics(&rel).await
        );
        insta::assert_yaml_snapshot!(
            name(fixture, &rel, "symbols"),
            session.document_symbols(&rel).await
        );
        insta::assert_yaml_snapshot!(
            name(fixture, &rel, "tokens"),
            session.semantic_tokens(&rel).await
        );
        insta::assert_yaml_snapshot!(
            name(fixture, &rel, "inlay"),
            session.inlay_hints(&rel).await
        );
        insta::assert_yaml_snapshot!(
            name(fixture, &rel, "format"),
            session.formatting(&rel).await
        );
    }
    insta::assert_yaml_snapshot!(
        name(fixture, "_workspace", "symbols"),
        session.workspace_symbols("").await
    );
}

fn name(fixture: &str, rel: &str, feature: &str) -> String {
    let slug = rel.replace(['/', '.'], "_");
    format!("{fixture}__{slug}__{feature}")
}
