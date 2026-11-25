//! Records whose layout would depend on the target do not compile.
#![cfg(feature = "derive")]

#[test]
fn pointer_sized_fields_are_rejected() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
