use tree_sitter::Node;

use super::{named_child_nodes, Formatter};

impl<'a> Formatter<'a> {
    pub(super) fn format_func_sig(&mut self, func_node: Node) {
        let params = self.child_of_kind(func_node, "func_params");
        if let Some(fp) = params {
            self.flush_comments_before(fp.start_byte());
        }
        // Keep `,` tokens in the walk so comments land on the correct side.
        let inner_nodes: Vec<Node> = self
            .child_of_kind(func_node, "func_params")
            .map(|fp| {
                let mut c = fp.walk();
                fp.children(&mut c)
                    .filter(|n| !n.is_missing() && n.kind() != "(" && n.kind() != ")")
                    .collect()
            })
            .unwrap_or_default();
        let rendered: Vec<String> = inner_nodes
            .iter()
            .map(|n| {
                if n.kind() == "func_param_group" {
                    self.render_param_group(*n)
                } else {
                    self.text(*n).to_string()
                }
            })
            .collect();

        let ret_str = self.return_type_suffix(func_node);

        let inline = {
            let mut s = String::from("(");
            for (i, r) in rendered.iter().enumerate() {
                if i > 0 && inner_nodes[i].kind() != "," {
                    s.push(' ');
                }
                s.push_str(r);
            }
            s.push(')');
            s
        };

        let group_count = inner_nodes
            .iter()
            .filter(|n| n.kind() == "func_param_group")
            .count();
        let fits = group_count == 0
            || self.current_line_len() + inline.len() + ret_str.len() <= self.line_limit;

        // The caller emits the `: type` so comments stay ordered - we do not.
        if fits {
            self.emit(&inline);
        } else {
            self.emit("(\n");
            self.level += 1;
            let mut emitted_groups = 0;
            for (node, r) in inner_nodes.iter().zip(rendered.iter()) {
                if node.kind() == "," {
                    continue;
                }
                self.emit_indent();
                self.emit(r);
                if node.kind() == "func_param_group" {
                    emitted_groups += 1;
                    if emitted_groups < group_count {
                        self.emit(",");
                    }
                }
                self.nl();
            }
            self.level -= 1;
            self.emit_indent();
            self.emit(")");
        }
        if let Some(fp) = params {
            self.consume_comments_before(fp.end_byte());
        }
    }

    fn collect_func_param_nodes<'t>(&self, func_node: Node<'t>) -> Vec<Node<'t>> {
        self.child_of_kind(func_node, "func_params")
            .map(named_child_nodes)
            .unwrap_or_default()
    }

    fn render_param_group(&self, node: Node) -> String {
        self.render_node(node)
    }

    fn return_type_suffix(&self, func_node: Node) -> String {
        let mut c = func_node.walk();
        let mut past_params = false;
        let mut past_colon = false;
        for child in func_node.children(&mut c) {
            if child.kind() == "func_params" {
                past_params = true;
            }
            if past_params && child.kind() == ":" && !child.is_missing() {
                past_colon = true;
            }
            if past_colon && child.kind() == "type_annot" {
                let colon = if self.compact_colon { ": " } else { " : " };
                return format!("{}{}", colon, self.text(child));
            }
        }
        String::new()
    }

    pub(super) fn render_sig(&self, func_node: Node) -> Option<String> {
        let param_groups: Vec<String> = self
            .collect_func_param_nodes(func_node)
            .into_iter()
            .filter(|n| n.kind() == "func_param_group")
            .map(|n| self.render_param_group(n))
            .collect();

        // Guard: no func_params means this isn't a callable we can render.
        self.child_of_kind(func_node, "func_params")?;

        let ret_str = self.return_type_suffix(func_node);

        Some(format!("({}){}", param_groups.join(", "), ret_str))
    }
}
