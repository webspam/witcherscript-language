use lsp_types::Url;

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
