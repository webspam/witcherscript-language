use std::collections::BTreeSet;
use std::fmt::Write;

use expect_test::expect_file;
use tree_sitter::Language;

fn render_kinds(language: &Language) -> String {
    let kind_count =
        u16::try_from(language.node_kind_count()).expect("tree-sitter kind ids are u16");
    let mut names: BTreeSet<&str> = (0..kind_count)
        .filter(|&id| {
            language.node_kind_is_named(id)
                && language.node_kind_is_visible(id)
                && !language.node_kind_is_supertype(id)
        })
        .filter_map(|id| language.node_kind_for_id(id))
        .collect();
    names.insert("ERROR");
    render_module("node kinds", &names)
}

fn render_fields(language: &Language) -> String {
    let field_count = u16::try_from(language.field_count()).expect("tree-sitter field ids are u16");
    let names: BTreeSet<&str> = (1..=field_count)
        .filter_map(|id| language.field_name_for_id(id))
        .collect();
    render_module("field names", &names)
}

fn render_module(what: &str, names: &BTreeSet<&str>) -> String {
    let header = format!(
        "//! Generated registry of grammar {what}. Do not edit.\n\
         //! Regenerate after a grammar bump: `UPDATE_EXPECT=1 cargo test sourcegen`\n\
         #![allow(dead_code)] // covers the whole grammar; unused consts are expected\n\n"
    );
    let consts: String = names.iter().fold(String::new(), |mut acc, s| {
        let _ = writeln!(
            acc,
            "pub(crate) const {}: &str = \"{s}\";",
            s.to_uppercase()
        );
        acc
    });
    header + &consts
}

#[test]
fn kinds_module_matches_grammar() {
    let language = tree_sitter_witcherscript::language();
    expect_file!["kinds.rs"].assert_eq(&render_kinds(&language));
}

#[test]
fn fields_module_matches_grammar() {
    let language = tree_sitter_witcherscript::language();
    expect_file!["fields.rs"].assert_eq(&render_fields(&language));
}
