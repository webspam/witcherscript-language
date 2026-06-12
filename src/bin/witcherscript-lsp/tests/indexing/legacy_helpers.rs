use std::path::{Path, PathBuf};

use lsp_types::Url;

use crate::backend::Backend;
use crate::tests::support::{LocalTempDir, make_backend};

pub(super) fn write_script(dir: &Path, rel: &str, contents: &str) -> PathBuf {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(&path, contents).expect("write script");
    path
}

pub(super) fn make_game_dir(temp: &Path, rel: &str, contents: &str) -> (PathBuf, Url) {
    let game_dir = temp.join("game");
    let full_rel = Path::new("content")
        .join("content0")
        .join("scripts")
        .join(rel);
    let path = write_script(&game_dir, full_rel.to_str().unwrap(), contents);
    let url = Url::from_file_path(&path).expect("base path -> url");
    (game_dir, url)
}

pub(super) async fn indexed_legacy_override(name: &str) -> (LocalTempDir, Backend, Url, Url) {
    let temp = LocalTempDir::new(name);
    let (game_dir, _base_url) =
        make_game_dir(temp.path(), "game/r4Player.ws", "class CR4Player {}\n");
    let legacy_dir = temp.path().join("legacy");
    let override_path = write_script(
        &legacy_dir,
        "game/r4Player.ws",
        "class CR4Player {}\n// legacy\n",
    );
    let new_path = write_script(&legacy_dir, "game/MyNewMod.ws", "class CMyNewMod {}\n");
    let override_url = Url::from_file_path(&override_path).expect("override path -> url");
    let new_url = Url::from_file_path(&new_path).expect("new path -> url");

    let backend = make_backend();
    backend.update_config(|c| {
        c.game_directory = Some(game_dir);
        c.legacy_script_dirs = vec![legacy_dir];
    });
    backend.index_base_scripts().await;
    (temp, backend, override_url, new_url)
}
