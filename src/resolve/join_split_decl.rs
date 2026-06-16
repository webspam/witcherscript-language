use crate::formatter::line_indent;

use super::body_model::{BodyModel, JoinTarget, SplitTarget};
use super::extract_common::{EditPlan, Splice, delete_statement};

pub fn join_declaration(model: &BodyModel, byte: usize) -> Option<EditPlan> {
    let (local, from_assignment) = if let Some(local) = model.local_at_declaration_stmt(byte) {
        (local, None)
    } else {
        let (local, stmt) = model.write_at(byte)?;
        (local, Some(stmt))
    };
    let JoinTarget {
        value,
        stmt,
        insert_at,
        confidence,
    } = model.joinable_assignment(local)?;

    // When the cursor is on an assignment, join that one rather than an earlier assignment.
    if from_assignment.is_some_and(|cursor_stmt| cursor_stmt != stmt) {
        return None;
    }

    let source = &model.document().source;
    let init = &source[value];
    Some(EditPlan {
        edits: vec![
            Splice {
                range: insert_at..insert_at,
                text: format!(" = {init}"),
            },
            delete_statement(source, stmt),
        ],
        confidence,
    })
}

pub fn split_declaration(model: &BodyModel, byte: usize) -> Option<EditPlan> {
    let local = model.local_at_declaration_stmt(byte)?;
    let decl = model.declaration(local)?;
    let SplitTarget {
        insert_at,
        confidence,
    } = model.splittable_declaration(local)?;

    let source = &model.document().source;
    let name = &source[decl.names[decl.target_index].clone()];
    let var_type = decl.var_type?;
    let init = decl.init?;
    let assignment = format!(
        "\n{indent}{name} = {value};",
        indent = line_indent(source, insert_at),
        value = &source[init.clone()],
    );
    Some(EditPlan {
        edits: vec![
            Splice {
                range: var_type.end..init.end,
                text: String::new(),
            },
            Splice {
                range: insert_at..insert_at,
                text: assignment,
            },
        ],
        confidence,
    })
}
