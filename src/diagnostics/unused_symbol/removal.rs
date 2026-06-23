use std::ops::Range;

use tree_sitter::Node;

use crate::cst::{fields, kinds};
use crate::resolve::{delete_statement, remove_list_entry};

pub(super) fn separator(node: Node<'_>) -> Range<usize> {
    let prev = node
        .prev_sibling()
        .filter(|n| n.kind() == ",")
        .and_then(|comma| comma.prev_sibling())
        .map(|n| n.byte_range());
    let next = node
        .next_sibling()
        .filter(|n| n.kind() == ",")
        .and_then(|comma| comma.next_sibling())
        .map(|n| n.byte_range());
    remove_list_entry(&node.byte_range(), prev.as_ref(), next.as_ref()).range
}

pub(super) fn statement(source: &str, node: Node<'_>) -> Range<usize> {
    delete_statement(source, node.byte_range()).range
}

// A `default`/`hint` for a removed field would dangle, so delete those entries too.
pub(super) fn field_defaults(source: &str, field: Node<'_>, names: &[&str]) -> Vec<Range<usize>> {
    let Some(body) = field.parent() else {
        return Vec::new();
    };
    let mut cursor = body.walk();
    let mut ranges = Vec::new();
    for child in body.children(&mut cursor) {
        match child.kind() {
            kinds::MEMBER_DEFAULT_VAL | kinds::MEMBER_HINT => {
                push_if_targeted(&mut ranges, source, child, names);
            }
            kinds::MEMBER_DEFAULT_VAL_BLOCK => {
                let mut block_cursor = child.walk();
                for assign in child
                    .children(&mut block_cursor)
                    .filter(|a| a.kind() == kinds::MEMBER_DEFAULT_VAL_BLOCK_ASSIGN)
                {
                    push_if_targeted(&mut ranges, source, assign, names);
                }
            }
            _ => {}
        }
    }
    ranges
}

fn push_if_targeted(out: &mut Vec<Range<usize>>, source: &str, node: Node<'_>, names: &[&str]) {
    let Some(member) = node.child_by_field_name(fields::MEMBER) else {
        return;
    };
    let Ok(name) = member.utf8_text(source.as_bytes()) else {
        return;
    };
    if names.contains(&name) {
        out.push(delete_statement(source, node.byte_range()).range);
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::*;
    use crate::cst::nav::decl_name_idents;
    use crate::document::parse_document;

    fn find_local_var_decl(node: Node<'_>) -> Option<Node<'_>> {
        if node.kind() == kinds::LOCAL_VAR_DECL_STMT {
            return Some(node);
        }
        let mut cursor = node.walk();
        node.children(&mut cursor).find_map(find_local_var_decl)
    }

    #[test]
    fn removes_each_name_from_a_gnarly_grouped_declaration() {
        let src = "\
function F() {
    var a,
   b
     ,c,
    d
    ,e,
      f
       : int;
}
";
        let doc = parse_document(src).expect("gnarly declaration parses");
        let decl = find_local_var_decl(doc.tree.root_node()).expect("a local var declaration");

        let mut report = String::new();
        for ident in decl_name_idents(decl) {
            let name = ident.utf8_text(src.as_bytes()).expect("name is utf-8");
            let mut edited = src.to_string();
            edited.replace_range(separator(ident), "");
            let _ = writeln!(report, "=== remove `{name}` ===\n{edited}");
        }
        insta::assert_snapshot!(report);
    }
}
