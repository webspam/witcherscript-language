//! Access-level matrix: pins how field/method references at each access level are dispositioned
//! when extracted to a global function versus a sibling method. A global function reaches only
//! public members (through a receiver) and must promote or refuse the rest; a sibling method
//! shares `this` and reaches every level verbatim.

use rstest::rstest;

use super::super::{BodyModel, extract_function, extract_method};
use crate::formatter::FormatOptions;
use crate::test_support::TestDb;

#[derive(Clone, Copy)]
enum Access {
    Public,
    Protected,
    Private,
}

impl Access {
    fn keyword(self) -> &'static str {
        match self {
            Access::Public => "",
            Access::Protected => "protected ",
            Access::Private => "private ",
        }
    }
}

#[derive(Clone, Copy)]
enum Member {
    Field,
    Method,
}

#[derive(Clone, Copy)]
enum Which {
    Function,
    Method,
}

struct Scenario {
    src: String,
    bare: &'static str,
    qualified: &'static str,
}

fn scenario(access: Access, member: Member) -> Scenario {
    match member {
        Member::Field => Scenario {
            src: format!(
                "class CFoo {{\n    {}var fld : int;\n    function M() {{\n        var r : int;\n        r = fld + 1;\n    }}\n}}\n",
                access.keyword()
            ),
            bare: "fld + 1",
            qualified: "foo.fld + 1",
        },
        Member::Method => Scenario {
            src: format!(
                "class CFoo {{\n    {}function Helper() : int {{ return 1; }}\n    function M() {{\n        var r : int;\n        r = Helper() + 1;\n    }}\n}}\n",
                access.keyword()
            ),
            bare: "Helper() + 1",
            qualified: "foo.Helper() + 1",
        },
    }
}

fn apply(src: &str, needle: &str, which: Which) -> Option<String> {
    let t = TestDb::new(src);
    let uri = t.primary_uri();
    let doc = t.doc_for(uri);
    let start = doc.source.find(needle).expect("needle present");
    let range = start..start + needle.len();
    let options = FormatOptions::default();
    let db = t.db();
    let model = BodyModel::enclosing(uri, doc, &db, start)?;
    let extraction = match which {
        Which::Function => extract_function(&model, range, options),
        Which::Method => extract_method(&model, range, options),
    }?;
    Some(extraction.plan.apply(&doc.source))
}

#[rstest]
#[case::public_field(Access::Public, Member::Field)]
#[case::protected_field(Access::Protected, Member::Field)]
#[case::private_field(Access::Private, Member::Field)]
#[case::public_method(Access::Public, Member::Method)]
#[case::protected_method(Access::Protected, Member::Method)]
#[case::private_method(Access::Private, Member::Method)]
fn method_keeps_every_access_level_verbatim(#[case] access: Access, #[case] member: Member) {
    let s = scenario(access, member);
    let applied =
        apply(&s.src, s.bare, Which::Method).expect("a sibling method extracts at every level");
    assert!(
        applied.contains("private function NewMethod() : int {"),
        "no member becomes a parameter, got:\n{applied}"
    );
    assert!(
        applied.contains(&format!("return {};", s.bare)),
        "member text is unchanged in the body, got:\n{applied}"
    );
    assert!(
        applied.contains("r = NewMethod();"),
        "the call passes no captured member, got:\n{applied}"
    );
    assert!(
        !applied.contains("foo"),
        "no receiver is synthesised for a method, got:\n{applied}"
    );
}

#[rstest]
#[case::public_field_routes_through_receiver(Access::Public, Member::Field)]
#[case::public_method_routes_through_receiver(Access::Public, Member::Method)]
fn function_routes_public_member_through_a_receiver(
    #[case] access: Access,
    #[case] member: Member,
) {
    let s = scenario(access, member);
    let applied = apply(&s.src, s.bare, Which::Function).expect("public members extract");
    assert!(
        applied.contains("function NewFunction(foo : CFoo) : int {"),
        "public access arrives through a receiver parameter, got:\n{applied}"
    );
    assert!(
        applied.contains(&format!("return {};", s.qualified)),
        "the member is qualified by the receiver, got:\n{applied}"
    );
    assert!(
        applied.contains("r = NewFunction(this);"),
        "the call forwards this as the receiver, got:\n{applied}"
    );
}

#[rstest]
#[case::protected_field_is_promoted(Access::Protected)]
#[case::private_field_is_promoted(Access::Private)]
fn function_promotes_a_non_public_field_to_a_parameter(#[case] access: Access) {
    let s = scenario(access, Member::Field);
    let applied = apply(&s.src, s.bare, Which::Function).expect("a non-public field is passed in");
    assert!(
        applied.contains("function NewFunction(fld : int) : int {"),
        "the field becomes a value parameter, got:\n{applied}"
    );
    assert!(
        applied.contains("return fld + 1;"),
        "the field is referenced bare inside the function, got:\n{applied}"
    );
    assert!(
        applied.contains("r = NewFunction(fld);"),
        "the call passes the field by value, got:\n{applied}"
    );
}

#[rstest]
#[case::protected_method_is_unreachable(Access::Protected)]
#[case::private_method_is_unreachable(Access::Private)]
fn function_refuses_a_non_public_method(#[case] access: Access) {
    let s = scenario(access, Member::Method);
    assert!(
        apply(&s.src, s.bare, Which::Function).is_none(),
        "a global function cannot reach a non-public method"
    );
}

fn written_field_src(access: Access) -> String {
    format!(
        "class CFoo {{\n    {}var fld : int;\n    function M() {{\n        fld = fld + 1;\n    }}\n}}\n",
        access.keyword()
    )
}

const WRITTEN: &str = "fld = fld + 1;";

#[rstest]
#[case::public(Access::Public)]
#[case::protected(Access::Protected)]
#[case::private(Access::Private)]
fn method_keeps_a_written_field_verbatim(#[case] access: Access) {
    let applied = apply(&written_field_src(access), WRITTEN, Which::Method)
        .expect("a sibling method extracts a written field at every level");
    assert!(
        applied.contains("private function NewMethod() {"),
        "the method is void with no parameters, got:\n{applied}"
    );
    assert!(
        applied.contains("        fld = fld + 1;\n"),
        "the write is unchanged, got:\n{applied}"
    );
    assert!(
        applied.contains("NewMethod();"),
        "the field is not promoted to an out parameter, got:\n{applied}"
    );
    assert!(
        !applied.contains("foo") && !applied.contains("out "),
        "no receiver or out promotion, got:\n{applied}"
    );
}

#[test]
fn function_routes_a_written_public_field_through_the_receiver() {
    let applied = apply(&written_field_src(Access::Public), WRITTEN, Which::Function)
        .expect("a written public field extracts");
    assert!(
        applied.contains("function NewFunction(foo : CFoo) {"),
        "public access arrives through a receiver, got:\n{applied}"
    );
    assert!(
        applied.contains("foo.fld = foo.fld + 1;"),
        "the write is routed through the receiver, got:\n{applied}"
    );
    assert!(
        applied.contains("NewFunction(this);"),
        "the call forwards this, got:\n{applied}"
    );
}

#[rstest]
#[case::protected(Access::Protected)]
#[case::private(Access::Private)]
fn function_promotes_a_written_non_public_field_to_out(#[case] access: Access) {
    let applied = apply(&written_field_src(access), WRITTEN, Which::Function)
        .expect("a written non-public field extracts");
    assert!(
        applied.contains("function NewFunction(out fld : int) {"),
        "the written field becomes an out parameter, got:\n{applied}"
    );
    assert!(
        applied.contains("    fld = fld + 1;\n"),
        "the field is referenced bare inside the function, got:\n{applied}"
    );
    assert!(
        applied.contains("NewFunction(fld);"),
        "the call passes the field by reference, got:\n{applied}"
    );
}

// Access is read from the inherited declaration, not the use site: protected members of a base
// are reachable by a sibling method (verbatim) but not by a global function (promote / refuse).
#[rstest]
#[case::inherited_protected_field(
    "class Base {\n    protected var fld : int;\n}\nclass CFoo extends Base {\n    function M() {\n        var r : int;\n        r = fld + 1;\n    }\n}\n",
    "fld + 1",
    Member::Field
)]
#[case::inherited_protected_method(
    "class Base {\n    protected function Helper() : int { return 1; }\n}\nclass CFoo extends Base {\n    function M() {\n        var r : int;\n        r = Helper() + 1;\n    }\n}\n",
    "Helper() + 1",
    Member::Method
)]
fn inherited_protected_member_obeys_the_same_rules(
    #[case] src: &str,
    #[case] needle: &str,
    #[case] member: Member,
) {
    let method = apply(src, needle, Which::Method)
        .expect("a sibling method reaches inherited protected members");
    assert!(
        method.contains(&format!("return {needle};")) && method.contains("NewMethod() : int {"),
        "inherited protected member moves verbatim into a method, got:\n{method}"
    );
    match member {
        Member::Field => {
            let function =
                apply(src, needle, Which::Function).expect("inherited protected field is promoted");
            assert!(
                function.contains("function NewFunction(fld : int) : int {"),
                "inherited protected field is promoted for a global function, got:\n{function}"
            );
        }
        Member::Method => assert!(
            apply(src, needle, Which::Function).is_none(),
            "a global function cannot reach an inherited protected method"
        ),
    }
}
