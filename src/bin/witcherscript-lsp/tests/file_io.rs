use witcherscript_language::files::read_script_file;

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

fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, bytes).expect("temp file write should succeed");
    path
}

#[test]
fn reads_utf8_script_file() {
    let path = write_temp("ws_test_utf8.ws", b"class CExample {}\n");
    assert_eq!(
        read_script_file(&path).expect("should read"),
        "class CExample {}\n"
    );
}

#[test]
fn reads_utf16le_script_file() {
    let bytes = encode_utf16le("class CExample {}\n");
    let path = write_temp("ws_test_utf16le.ws", &bytes);
    assert_eq!(
        read_script_file(&path).expect("should read"),
        "class CExample {}\n"
    );
}

#[test]
fn reads_utf16be_script_file() {
    let bytes = encode_utf16be("class CExample {}\n");
    let path = write_temp("ws_test_utf16be.ws", &bytes);
    assert_eq!(
        read_script_file(&path).expect("should read"),
        "class CExample {}\n"
    );
}

#[test]
fn returns_error_for_invalid_utf8() {
    let path = write_temp("ws_test_bad.ws", &[0x80, 0x81, 0x82]);
    assert!(read_script_file(&path).is_err());
}
