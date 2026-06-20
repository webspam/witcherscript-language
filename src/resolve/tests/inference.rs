use rstest::rstest;

use super::super::inference::infer_type;
use crate::test_support::TestDb;
use crate::types::{Primitive, Type};

fn inferred(fixture: &str, needle: &str) -> Type {
    let t = TestDb::new(fixture);
    let uri = t.primary_uri();
    let doc = t.doc_for(uri);
    let start = doc
        .source
        .find(needle)
        .unwrap_or_else(|| panic!("needle {needle:?} not found in fixture"));
    let node = doc
        .tree
        .root_node()
        .descendant_for_byte_range(start, start + needle.len())
        .unwrap_or_else(|| panic!("no node covering needle {needle:?}"));
    infer_type(uri, doc, &t.db(), node, start)
}

const INT_OPERANDS: &str = "function F() {\n var a : int;\n var b : int;\n var r : bool;\n";
const BOOL_OPERANDS: &str = "function F() {\n var a : bool;\n var b : bool;\n var r : bool;\n";

#[rstest]
#[case::eq("==", INT_OPERANDS)]
#[case::neq("!=", INT_OPERANDS)]
#[case::lt("<", INT_OPERANDS)]
#[case::le("<=", INT_OPERANDS)]
#[case::gt(">", INT_OPERANDS)]
#[case::ge(">=", INT_OPERANDS)]
#[case::and("&&", BOOL_OPERANDS)]
#[case::or("||", BOOL_OPERANDS)]
fn comparison_and_logical_ops_yield_bool(#[case] op: &str, #[case] prelude: &str) {
    let needle = format!("a {op} b");
    let fixture = format!("{prelude} r = {needle};\n}}\n");
    assert_eq!(
        inferred(&fixture, &needle),
        Type::Primitive(Primitive::Bool),
        "operator {op} should yield bool"
    );
}

#[rstest]
#[case::bitor("|")]
#[case::bitand("&")]
#[case::bitxor("^")]
fn bitwise_ops_yield_int(#[case] op: &str) {
    let needle = format!("a {op} b");
    let fixture = format!(
        "function F() {{\n var a : int;\n var b : int;\n var r : int;\n r = {needle};\n}}\n"
    );
    assert_eq!(
        inferred(&fixture, &needle),
        Type::Primitive(Primitive::Int),
        "operator {op} should yield int"
    );
}

#[rstest]
#[case::string_plus_int("s + i")]
#[case::int_plus_string("i + s")]
fn string_concat_yields_string(#[case] needle: &str) {
    let fixture = format!(
        "function F() {{\n var s : string;\n var i : int;\n var r : string;\n r = {needle};\n}}\n"
    );
    assert_eq!(
        inferred(&fixture, needle),
        Type::Primitive(Primitive::String),
        "concat {needle} should yield string"
    );
}

#[test]
fn name_plus_name_yields_string() {
    let fixture =
        "function F() {\n var n1 : name;\n var n2 : name;\n var r : string;\n r = n1 + n2;\n}\n";
    assert_eq!(
        inferred(fixture, "n1 + n2"),
        Type::Primitive(Primitive::String),
        "name + name should yield string"
    );
}

#[rstest]
#[case::int_plus_int("i + j", Type::Primitive(Primitive::Int))]
#[case::float_plus_int("f + i", Type::Primitive(Primitive::Float))]
#[case::int_minus_float("i - f", Type::Primitive(Primitive::Float))]
#[case::int_plus_byte("i + y", Type::Primitive(Primitive::Int))]
#[case::float_times_float("f * g", Type::Primitive(Primitive::Float))]
#[case::int_mod_int("i % j", Type::Primitive(Primitive::Int))]
#[case::string_times_int("s * i", Type::Unknown)]
fn arithmetic_join_rules(#[case] needle: &str, #[case] expected: Type) {
    let fixture = format!(
        concat!(
            "function F() {{\n",
            " var i : int;\n var j : int;\n var y : byte;\n",
            " var f : float;\n var g : float;\n var s : string;\n",
            " var r : float;\n r = {needle};\n}}\n",
        ),
        needle = needle
    );
    assert_eq!(
        inferred(&fixture, needle),
        expected,
        "arithmetic {needle} join mismatch"
    );
}

#[rstest]
#[case::vec_plus_vec("a + b", Type::Named("Vector".to_string()))]
#[case::vec_div_float("a / f", Type::Named("Vector".to_string()))]
#[case::float_times_vec("f * a", Type::Named("Vector".to_string()))]
#[case::vec_times_vec("a * b", Type::Named("Vector".to_string()))]
#[case::mismatched_structs("a + o", Type::Unknown)]
fn struct_arithmetic_preserves_struct_type(#[case] needle: &str, #[case] expected: Type) {
    let fixture = format!(
        concat!(
            "struct Vector {{ var X : float; }}\n",
            "struct Other {{ var X : float; }}\n",
            "function F() {{\n",
            " var a : Vector;\n var b : Vector;\n var o : Other;\n var f : float;\n",
            " var r : Vector;\n r = {needle};\n}}\n",
        ),
        needle = needle
    );
    assert_eq!(
        inferred(&fixture, needle),
        expected,
        "struct arithmetic {needle} join mismatch"
    );
}

#[rstest]
#[case::enum_times_float("e * h")]
#[case::enum_plus_enum("e + e")]
fn enum_arithmetic_stays_unknown(#[case] needle: &str) {
    let fixture = format!(
        concat!(
            "enum E {{ A, B }}\n",
            "function F() {{\n",
            " var e : E;\n var h : float;\n",
            " var r : float;\n r = {needle};\n}}\n",
        ),
        needle = needle
    );
    assert_eq!(
        inferred(&fixture, needle),
        Type::Unknown,
        "enum arithmetic {needle} should stay Unknown, not infer the enum type"
    );
}

#[test]
fn unary_not_yields_bool() {
    let fixture = "function F() {\n var a : bool;\n var r : bool;\n r = !a;\n}\n";
    assert_eq!(
        inferred(fixture, "!a"),
        Type::Primitive(Primitive::Bool),
        "!a should yield bool"
    );
}

#[test]
fn double_negation_yields_bool() {
    let fixture = "function F() {\n var a : bool;\n var r : bool;\n r = !(!a);\n}\n";
    assert_eq!(
        inferred(fixture, "!(!a)"),
        Type::Primitive(Primitive::Bool),
        "!(!a) should yield bool"
    );
}

#[rstest]
#[case::neg_int("-i", Type::Primitive(Primitive::Int))]
#[case::neg_float("-f", Type::Primitive(Primitive::Float))]
#[case::plus_int("+i", Type::Primitive(Primitive::Int))]
fn unary_sign_preserves_operand_type(#[case] needle: &str, #[case] expected: Type) {
    let fixture = format!(
        "function F() {{\n var i : int;\n var f : float;\n var r : float;\n r = {needle};\n}}\n"
    );
    assert_eq!(
        inferred(&fixture, needle),
        expected,
        "unary {needle} should preserve operand type"
    );
}

#[test]
fn unary_bitnot_yields_int() {
    let fixture = "function F() {\n var i : int;\n var r : int;\n r = ~i;\n}\n";
    assert_eq!(
        inferred(fixture, "~i"),
        Type::Primitive(Primitive::Int),
        "~i should yield int"
    );
}

#[test]
fn ternary_stays_unknown() {
    let fixture = "function F() {\n var c : bool;\n var a : int;\n var b : int;\n var r : int;\n r = c ? a : b;\n}\n";
    assert_eq!(
        inferred(fixture, "c ? a : b"),
        Type::Unknown,
        "ternary is not valid WitcherScript; inference must stay unknown"
    );
}

#[test]
fn super_member_call_infers_base_return_type() {
    let fixture = "class B {\n    function Hit() : int { return 1; }\n}\nclass C extends B {\n    function M() {\n        var r : int;\n        r = super.Hit();\n    }\n}\n";
    assert_eq!(
        inferred(fixture, "super.Hit()"),
        Type::Primitive(Primitive::Int),
        "super resolves to the base class, so super.Hit() is the base method's return type"
    );
}

#[test]
fn parent_member_call_infers_owner_return_type() {
    let fixture = "class Owner {\n    function Ping() : int { return 1; }\n}\nstate S in Owner {\n    function M() {\n        var r : int;\n        r = parent.Ping();\n    }\n}\n";
    assert_eq!(
        inferred(fixture, "parent.Ping()"),
        Type::Primitive(Primitive::Int),
        "parent resolves to the owner class, so parent.Ping() is the owner method's return type"
    );
}

#[test]
fn virtual_parent_member_call_infers_owner_return_type() {
    let fixture = "class Owner {\n    function Ping() : int { return 1; }\n}\nstate S in Owner {\n    function M() {\n        var r : int;\n        r = virtual_parent.Ping();\n    }\n}\n";
    assert_eq!(
        inferred(fixture, "virtual_parent.Ping()"),
        Type::Primitive(Primitive::Int),
        "virtualParent resolves to the owner class, the same as parent"
    );
}

const ARRAY_OF_STRUCTS: &str = concat!(
    "struct Handle {\n",
    "    var id : int;\n",
    "}\n",
    "struct Aspect {\n",
    "    var projTemplate : Handle;\n",
    "}\n",
    "function F() {\n",
    "    var aspects : array<Aspect>;\n",
    "    var fireMode : int;\n",
    "    var t : Handle;\n",
    "    t = aspects[fireMode].projTemplate;\n",
    "}\n",
);

#[test]
fn indexing_array_of_structs_yields_element_struct() {
    assert_eq!(
        inferred(ARRAY_OF_STRUCTS, "aspects[fireMode]"),
        Type::Named("Aspect".to_string()),
        "indexing array<Aspect> must yield the element struct type"
    );
}

#[test]
fn member_access_on_indexed_struct_infers_field_type() {
    assert_eq!(
        inferred(ARRAY_OF_STRUCTS, "aspects[fireMode].projTemplate"),
        Type::Named("Handle".to_string()),
        "accessing a field on an indexed array element must infer the field's type"
    );
}
