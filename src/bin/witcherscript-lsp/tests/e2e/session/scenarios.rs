use super::battery::snapshot_battery;
use super::{EditorSession, WorkspaceFixture};

#[tokio::test]
async fn minimal_workspace_battery() {
    let mut session = EditorSession::open(WorkspaceFixture::Minimal).await;
    snapshot_battery(&mut session, "minimal").await;
}

#[tokio::test]
async fn minimal_positional_probes() {
    let mut session = EditorSession::open(WorkspaceFixture::Minimal).await;
    let rel = "scripts/player.ws";
    insta::assert_yaml_snapshot!("minimal_player_hover", session.hover(rel).await);
    insta::assert_yaml_snapshot!("minimal_player_definition", session.definition(rel).await);
    insta::assert_yaml_snapshot!(
        "minimal_player_type_definition",
        session.type_definition(rel).await
    );
    insta::assert_yaml_snapshot!("minimal_player_references", session.references(rel).await);
    insta::assert_yaml_snapshot!("minimal_player_completion", session.completion(rel).await);
    insta::assert_yaml_snapshot!("minimal_player_highlights", session.highlights(rel).await);
    insta::assert_yaml_snapshot!(
        "minimal_player_signature_help",
        session.signature_help(rel).await
    );
}

#[tokio::test]
async fn base_layering_battery() {
    let mut session = EditorSession::open(WorkspaceFixture::BaseLayering).await;
    snapshot_battery(&mut session, "base_layering").await;
}

#[tokio::test]
async fn base_layering_resolves_into_base_scripts() {
    let mut session = EditorSession::open(WorkspaceFixture::BaseLayering).await;
    let definition = session.definition("mod/scripts/mod_player.ws").await;
    insta::assert_yaml_snapshot!("base_layering_definition_into_base", definition);
    assert!(
        definition.iter().any(|loc| loc.file.contains("base.ws")),
        "mod method call must resolve into the base script, got {definition:?}"
    );
}

#[tokio::test]
async fn multi_root_battery() {
    let mut session = EditorSession::open(WorkspaceFixture::MultiRoot).await;
    snapshot_battery(&mut session, "multi_root").await;
}

#[tokio::test]
async fn multi_root_resolves_across_roots() {
    let mut session = EditorSession::open(WorkspaceFixture::MultiRoot).await;
    let definition = session.definition("rootB/scripts/b.ws").await;
    insta::assert_yaml_snapshot!("multi_root_definition_across_roots", definition);
    assert!(
        definition.iter().any(|loc| loc.file.contains("rootA")),
        "call in rootB must resolve to the class declared in rootA, got {definition:?}"
    );
}

#[tokio::test]
async fn editing_a_file_reports_new_diagnostics() {
    let mut session = EditorSession::open(WorkspaceFixture::Minimal).await;
    let rel = "scripts/types.ws";
    session.edit(rel, 2, "class CWeapon {\n").await;
    let diagnostics = session.diagnostics(rel).await;
    assert!(
        !diagnostics.is_empty(),
        "an unclosed class introduced by an edit must report a diagnostic"
    );
}
