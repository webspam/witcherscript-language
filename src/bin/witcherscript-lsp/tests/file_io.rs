use rstest::rstest;
use witcherscript_language::files::read_text_file;

use crate::tests::support::LocalTempDir;

fn encode_utf16le(s: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE]; // BOM
    for unit in s.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn encode_utf16be(s: &str) -> Vec<u8> {
    let mut bytes = vec![0xFE, 0xFF]; // BOM
    for unit in s.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    bytes
}

fn read_temp(dir_name: &str, bytes: &[u8]) -> std::io::Result<String> {
    let temp = LocalTempDir::new(dir_name);
    let path = temp.path().join("script.ws");
    std::fs::write(&path, bytes).expect("temp file write should succeed");
    read_text_file(&path)
}

#[rstest]
#[case::utf8("ws_test_utf8", b"class CExample {}\n".to_vec(), Some("class CExample {}\n"))]
#[case::utf16le(
    "ws_test_utf16le",
    encode_utf16le("class CExample {}\n"),
    Some("class CExample {}\n")
)]
#[case::utf16be(
    "ws_test_utf16be",
    encode_utf16be("class CExample {}\n"),
    Some("class CExample {}\n")
)]
#[case::invalid_utf8("ws_test_bad", vec![0x80, 0x81, 0x82], None)]
fn read_text_file_decodes_or_errors(
    #[case] dir_name: &str,
    #[case] bytes: Vec<u8>,
    #[case] expected: Option<&str>,
) {
    let got = read_temp(dir_name, &bytes);
    match expected {
        Some(text) => assert_eq!(got.expect("should read"), text),
        None => assert!(got.is_err()),
    }
}
