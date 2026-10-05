mod common;

use common::input;
use serde_json::json;
use xc_script::{assert, AssertionOutcome, AssertionSpec, Phase};

fn spec(expression: &str, operator: &str, value: Option<&str>) -> AssertionSpec {
    AssertionSpec { expression: expression.into(), operator: operator.into(), value: value.map(str::to_owned) }
}

fn check(specs: &[AssertionSpec]) -> Vec<AssertionOutcome> {
    assert(specs, input(Phase::Tests, "")).unwrap()
}

fn one(expression: &str, operator: &str, value: Option<&str>) -> AssertionOutcome {
    check(&[spec(expression, operator, value)]).remove(0)
}

fn passes(expression: &str, operator: &str, value: Option<&str>) -> bool {
    one(expression, operator, value).passed
}

#[test]
fn ef_tst_02_equality_is_strict_and_the_operand_is_typed() {
    assert!(passes("res.status", "eq", Some("200")));
    assert!(!passes("res.headers['x-id']", "eq", Some("7")), "l'en-tête est le texte 7, l'opérande le nombre 7");
    let failed = one("res.status", "eq", Some("404"));
    assert!(!failed.passed);
    assert_eq!(failed.error.as_deref(), Some("expected 200 to equal 404"));
    assert_eq!(failed.actual, "200");
    assert!(!passes("res.status", "eq", Some("\"200\"")), "une chaîne entre guillemets n'est pas le nombre 200");
    assert!(passes("res.body.name", "eq", Some("Ada")));
    assert!(passes("res.body.name", "eq", Some("'Ada'")));
    assert!(passes("res.status", "neq", Some("404")));
}

#[test]
fn ef_tst_02_comparisons_need_numbers_like_chai() {
    assert!(passes("res.status", "gt", Some("199")));
    assert!(passes("res.status", "gte", Some("200")));
    assert!(passes("res.status", "lt", Some("300")));
    assert!(passes("res.status", "lte", Some("200")));
    let text = one("res.body.name", "gt", Some("1"));
    assert!(!text.passed, "{text:?}");
    assert!(passes("res.status", "between", Some("200, 299")));
    assert!(!passes("res.status", "between", Some("300,399")));
}

#[test]
fn ef_tst_02_in_contains_length_and_text_operators() {
    assert!(passes("res.status", "in", Some("[200, 201]")));
    assert!(passes("res.status", "notIn", Some("404,500")));
    assert!(passes("res.body.name", "contains", Some("d")));
    assert!(passes("res.body.items", "length", Some("2")));
    assert!(passes("res.body.items.length", "eq", Some("2")));
    assert!(passes("res.body.name", "startsWith", Some("A")));
    assert!(passes("res.body.name", "endsWith", Some("da")));
    assert!(!passes("res.body.name", "startsWith", Some("d")));
    assert!(passes("res.body.name", "matches", Some("/^A.a$/")));
    assert!(passes("res.body.name", "notMatches", Some("^z")));
    assert!(!passes("res.body.missing", "matches", Some("x")));
}

#[test]
fn ef_tst_02_unary_operators_follow_chai_strictly() {
    assert!(passes("res.body.name", "isString", None));
    assert!(passes("res.status", "isNumber", None));
    assert!(passes("res.body.items", "isArray", None));
    assert!(passes("res.body", "isJson", None));
    assert!(passes("res.body.missing", "isUndefined", None));
    assert!(passes("res.body.name", "isDefined", None));
    assert!(passes("res.body.items", "isNotEmpty", None));
    assert!(passes("res.body.items.length === 2", "isTruthy", None));
    assert!(!passes("'abc'", "isTruthy", None), "isTruthy est `true` strict, pas la vérité JS");
    assert!(passes("res.status === 404", "isFalsy", None));
    assert!(passes("null", "isNull", None));
}

#[test]
fn ef_tst_02_the_left_side_is_javascript_over_the_response_and_the_variables() {
    assert!(passes("res.headers['x-id']", "eq", Some("\"7\"")));
    assert!(passes("res.headers['content-type']", "contains", Some("json")));
    assert!(passes("res('items.id').length", "eq", Some("2")));
    assert!(passes("retries", "eq", Some("3")), "les variables de l'environnement sont des variables de l'expression");
    assert!(passes("bru.getVar('x')", "eq", Some("\"1\"")));
    assert!(passes("res.responseTime", "lt", Some("1000")));
}

#[test]
fn ef_tst_02_the_right_side_is_interpolated_then_evaluated_as_a_template() {
    assert!(!passes("res.body.name", "eq", Some("{{who}}")), "{{who}} n'est défini nulle part et reste tel quel");
    assert!(passes("res.body.name", "eq", Some("${res.body.name}")));
    assert!(passes("res.body.name", "eq", Some("A${'d'}a")));
    assert!(!passes("res.body.name", "eq", Some("{{greeting}}Ada")));
    assert!(passes("res.body.items[0].id", "eq", Some("{{x}}")), "x vaut \"1\" mais l'opérande devient le nombre 1");
}

#[test]
fn ef_tst_02_a_broken_left_side_becomes_an_error_value_not_a_failure_by_itself() {
    let broken = one("res.body.a.b", "isUndefined", None);
    assert!(!broken.passed, "{broken:?}");
    assert!(passes("res.body.a.b", "isDefined", None));
}

#[test]
fn ef_tst_02_an_unknown_operator_fails_with_a_message() {
    let unknown = one("res.status", "nope", Some("1"));
    assert!(!unknown.passed);
    assert_eq!(unknown.error.as_deref(), Some("Unknown assertion operator: nope"));
}

#[test]
fn ef_tst_02_every_assertion_is_reported_in_order() {
    let all = check(&[
        spec("res.status", "eq", Some("200")),
        spec("res.status", "eq", Some("1")),
        spec("res.body.name", "isString", None),
    ]);
    assert_eq!(all.iter().map(|a| a.passed).collect::<Vec<_>>(), [true, false, true]);
    assert_eq!(all[0].value.as_deref(), Some("200"));
    assert_eq!(json!(all[2].value), json!(null));
}
