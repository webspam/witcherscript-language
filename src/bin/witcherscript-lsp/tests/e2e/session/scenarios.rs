use super::battery::snapshot_battery;
use super::{EditorSession, WorkspaceFixture, e2e_snapshots};

#[tokio::test]
async fn minimal_workspace_battery() {
    let _guard = e2e_snapshots().bind_to_scope();
    let mut session = EditorSession::open(WorkspaceFixture::Minimal).await;
    snapshot_battery(&mut session, "minimal", "Player").await;
}

#[tokio::test]
async fn minimal_positional_probes() {
    let _guard = e2e_snapshots().bind_to_scope();
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
    let _guard = e2e_snapshots().bind_to_scope();
    let mut session = EditorSession::open(WorkspaceFixture::BaseLayering).await;
    snapshot_battery(&mut session, "base_layering", "Player").await;
}

#[tokio::test]
async fn base_layering_resolves_into_base_scripts() {
    let _guard = e2e_snapshots().bind_to_scope();
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
    let _guard = e2e_snapshots().bind_to_scope();
    let mut session = EditorSession::open(WorkspaceFixture::MultiRoot).await;
    snapshot_battery(&mut session, "multi_root", "Shared").await;
}

#[tokio::test]
async fn multi_root_resolves_across_roots() {
    let _guard = e2e_snapshots().bind_to_scope();
    let mut session = EditorSession::open(WorkspaceFixture::MultiRoot).await;
    let definition = session.definition("rootB/scripts/b.ws").await;
    insta::assert_yaml_snapshot!("multi_root_definition_across_roots", definition);
    assert!(
        definition.iter().any(|loc| loc.file.contains("rootA")),
        "call in rootB must resolve to the class declared in rootA, got {definition:?}"
    );
}

#[tokio::test]
async fn clean_workspace_has_no_workspace_diagnostics() {
    let mut session = EditorSession::open(WorkspaceFixture::MultiRoot).await;
    let diagnostics = session.workspace_diagnostics().await;
    assert!(
        diagnostics.is_empty(),
        "a clean multi-root workspace must report no diagnostics, got {diagnostics:?}"
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

#[tokio::test]
async fn emitter_mod_battery() {
    let _guard = e2e_snapshots().bind_to_scope();
    let mut session = EditorSession::open(WorkspaceFixture::EmitterMod).await;
    snapshot_battery(&mut session, "emitter_mod", "Emitter").await;
}

#[tokio::test]
async fn emitter_mod_resolves_into_base_scripts() {
    let _guard = e2e_snapshots().bind_to_scope();
    let mut session = EditorSession::open(WorkspaceFixture::EmitterMod).await;
    let definition = session.definition("mod/scripts/probe_lookup.ws").await;
    insta::assert_yaml_snapshot!("emitter_mod_lookup_definition", definition);
    assert!(
        definition.iter().any(|loc| loc.file.contains("manager.ws")),
        "the cross-file call must resolve to its declaration, got {definition:?}"
    );
}

#[tokio::test]
async fn emitter_mod_positional_probes() {
    let _guard = e2e_snapshots().bind_to_scope();
    let mut session = EditorSession::open(WorkspaceFixture::EmitterMod).await;

    let lookup = "mod/scripts/probe_lookup.ws";
    insta::assert_yaml_snapshot!(
        "emitter_mod_lookup_type_definition",
        session.type_definition(lookup).await
    );
    insta::assert_yaml_snapshot!("emitter_mod_lookup_hover", session.hover(lookup).await);
    insta::assert_yaml_snapshot!(
        "emitter_mod_lookup_references",
        session.references(lookup).await
    );
    insta::assert_yaml_snapshot!(
        "emitter_mod_lookup_completion",
        session.completion(lookup).await
    );

    let signature = "mod/scripts/probe_signature.ws";
    insta::assert_yaml_snapshot!(
        "emitter_mod_signature_help",
        session.signature_help(signature).await
    );
    insta::assert_yaml_snapshot!(
        "emitter_mod_signature_completion",
        session.completion(signature).await
    );
}
