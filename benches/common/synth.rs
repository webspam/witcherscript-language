use lsp_types::Url;
use witcherscript_language::builtins::load_builtins_index;
use witcherscript_language::document::{ParsedDocument, parse_document};
use witcherscript_language::resolve::WorkspaceIndex;

#[allow(dead_code)]
pub const TARGET_URI: &str = "file:///synth/target.ws";

#[allow(dead_code)]
pub type WorkspaceFixture = (WorkspaceIndex, WorkspaceIndex, ParsedDocument);

#[allow(dead_code)]
pub const FILE_SIZE_SMALL: (usize, usize) = (2, 3);
#[allow(dead_code)]
pub const FILE_SIZE_MEDIUM: (usize, usize) = (10, 6);
#[allow(dead_code)]
pub const FILE_SIZE_LARGE: (usize, usize) = (50, 10);

#[allow(dead_code)]
pub const WORKSPACE_SIZE_SMALL: usize = 10;
#[allow(dead_code)]
pub const WORKSPACE_SIZE_MEDIUM: usize = 100;
#[allow(dead_code)]
pub const WORKSPACE_SIZE_LARGE: usize = 500;

#[allow(dead_code)]
pub fn synth_file(num_classes: usize, methods_per_class: usize) -> String {
    let mut out = String::with_capacity(num_classes * methods_per_class * 96);
    out.push_str("function Top0(a: int, b: int) : int { return a + b; }\n");
    out.push_str("function Top1() : void { Top0(1, 2); }\n\n");

    for class_idx in 0..num_classes {
        if class_idx == 0 {
            out.push_str(&format!("class Class{class_idx} {{\n"));
        } else {
            out.push_str(&format!(
                "class Class{class_idx} extends Class{} {{\n",
                class_idx - 1
            ));
        }
        out.push_str(&format!("  var field{class_idx}_a : int;\n"));
        out.push_str(&format!("  var field{class_idx}_b : string;\n"));
        for method_idx in 0..methods_per_class {
            out.push_str(&format!(
                "  function method{method_idx}(arg: int) : int {{\n"
            ));
            out.push_str(&format!("    var local{method_idx} : int = arg;\n"));
            out.push_str(&format!(
                "    this.field{class_idx}_a = local{method_idx};\n"
            ));
            if method_idx > 0 {
                out.push_str(&format!(
                    "    this.method{}(local{method_idx});\n",
                    method_idx - 1
                ));
            }
            out.push_str(&format!("    return local{method_idx};\n"));
            out.push_str("  }\n");
        }
        out.push_str("}\n\n");
    }
    out
}

#[allow(dead_code)]
pub fn synth_workspace(num_files: usize) -> Vec<(Url, String)> {
    (0..num_files)
        .map(|i| {
            let uri = Url::parse(&format!("file:///synth/file{i}.ws")).expect("synth uri parses");
            (uri, synth_file(4, 6))
        })
        .collect()
}

#[allow(dead_code)]
pub fn build_workspace() -> WorkspaceFixture {
    let mut workspace = WorkspaceIndex::default();
    for (uri, source) in synth_workspace(40) {
        let doc = parse_document(source).expect("synth source must parse");
        workspace.update_document(uri.to_string(), &doc);
    }
    let target_doc = parse_document(synth_file(6, 6)).expect("synth source must parse");
    workspace.update_document(TARGET_URI, &target_doc);
    let base = load_builtins_index();
    (workspace, base, target_doc)
}
