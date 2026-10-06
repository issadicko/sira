//! Traduction des scripts de l'API `bru` vers `pm.*`, pour l'export Postman : l'inverse de la table d'import.
//!
//! Bruno passe par un arbre syntaxique ; ici une table de remplacements couvre les appels courants (variables,
//! `test`, `expect`, `res.*`, `req.*`). Un script plus tordu garde ses appels `bru.*`, à relire dans Postman.

use std::sync::OnceLock;

use regex::Regex;

const REPLACEMENTS: &[(&str, &str)] = &[
    (r"\bbru\.getEnvVar\(", "pm.environment.get("),
    (r"\bbru\.setEnvVar\(", "pm.environment.set("),
    (r"\bbru\.getEnvName\(\)", "pm.environment.name"),
    (r"\bbru\.getVar\(", "pm.variables.get("),
    (r"\bbru\.setVar\(", "pm.variables.set("),
    (r"\bbru\.hasVar\(", "pm.variables.has("),
    (r"\bbru\.deleteVar\(", "pm.variables.unset("),
    (r"\bbru\.interpolate\(", "pm.variables.replaceIn("),
    (r"\bbru\.getCollectionVar\(", "pm.collectionVariables.get("),
    (r"\bbru\.setCollectionVar\(", "pm.collectionVariables.set("),
    (r"\bbru\.hasCollectionVar\(", "pm.collectionVariables.has("),
    (r"\bbru\.deleteCollectionVar\(", "pm.collectionVariables.unset("),
    (r"\bbru\.deleteAllCollectionVars\(", "pm.collectionVariables.clear("),
    (r"\bbru\.getAllCollectionVars\(", "pm.collectionVariables.toObject("),
    (r"\bbru\.getGlobalEnvVar\(", "pm.globals.get("),
    (r"\bbru\.setGlobalEnvVar\(", "pm.globals.set("),
    (r"\bbru\.runner\.stopExecution\(\)", "pm.execution.setNextRequest(null)"),
    (r"\bbru\.runner\.setNextRequest\(", "pm.execution.setNextRequest("),
    (r"\bbru\.setNextRequest\(", "pm.execution.setNextRequest("),
    (r"\bbru\.sendRequest\(", "pm.sendRequest("),
    (r"\bbru\.cookies\.jar\(", "pm.cookies.jar("),
    (r"\bres\.getStatus\(\)", "pm.response.code"),
    (r"\bres\.getStatusText\(\)", "pm.response.status"),
    (r"\bres\.getBody\(\)", "pm.response.json()"),
    (r"\bres\.getHeader\(", "pm.response.headers.get("),
    (r"\bres\.getResponseTime\(\)", "pm.response.responseTime"),
    (r"\bres\.status\b", "pm.response.code"),
    (r"\bres\.body\b", "pm.response.json()"),
    (r"\breq\.getUrl\(\)", "pm.request.url.toString()"),
    (r"\breq\.getMethod\(\)", "pm.request.method"),
    (r"\breq\.getHeader\(", "pm.request.headers.get("),
    (r"\breq\.getBody\(\)", "pm.request.body"),
];

/// `test(` et `expect(` : seulement quand ils ne suivent ni un point ni un caractère de mot (`pm.test(`, `latest(`).
const BARE_CALLS: &[(&str, &str)] = &[(r"(^|[^.\w])test\(", "${1}pm.test("), (r"(^|[^.\w])expect\(", "${1}pm.expect(")];

fn compiled() -> &'static [(Regex, &'static str)] {
    static TABLE: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        REPLACEMENTS
            .iter()
            .chain(BARE_CALLS)
            .filter_map(|(pattern, replacement)| Regex::new(pattern).ok().map(|regex| (regex, *replacement)))
            .collect()
    })
}

/// Le script avec ses appels `bru`, `req` et `res` écrits à la façon de Postman.
pub fn translate(code: &str) -> String {
    compiled()
        .iter()
        .fold(code.to_owned(), |code, (regex, replacement)| regex.replace_all(&code, *replacement).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ef_imp_03_every_replacement_of_the_table_compiles() {
        assert_eq!(compiled().len(), REPLACEMENTS.len() + BARE_CALLS.len());
    }

    #[test]
    fn ef_imp_03_variables_tests_and_responses_become_their_postman_equivalents() {
        let code = "bru.setEnvVar('token', res.getBody().token);\ntest('ok', function () { expect(res.getStatus()).to.equal(200); });\nconst v = bru.getVar('a') + bru.getCollectionVar('b');";
        assert_eq!(
            translate(code),
            "pm.environment.set('token', pm.response.json().token);\npm.test('ok', function () { pm.expect(pm.response.code).to.equal(200); });\nconst v = pm.variables.get('a') + pm.collectionVariables.get('b');"
        );
    }

    #[test]
    fn ef_imp_03_response_and_request_accessors_are_rewritten() {
        assert_eq!(
            translate("res.status + res.body.id + res.getHeader('x')"),
            "pm.response.code + pm.response.json().id + pm.response.headers.get('x')"
        );
        assert_eq!(translate("req.getUrl() + req.getMethod()"), "pm.request.url.toString() + pm.request.method");
    }

    #[test]
    fn ef_imp_03_runner_control_calls_use_the_execution_api() {
        assert_eq!(translate("bru.runner.stopExecution();"), "pm.execution.setNextRequest(null);");
        assert_eq!(translate("bru.runner.setNextRequest('Login');"), "pm.execution.setNextRequest('Login');");
    }

    #[test]
    fn ef_imp_03_test_and_expect_are_left_alone_after_a_dot_or_inside_a_word() {
        assert_eq!(
            translate("pm.test('a'); latest(1); chai.expect(1); x.test('b')"),
            "pm.test('a'); latest(1); chai.expect(1); x.test('b')"
        );
        assert_eq!(translate("test('a');expect(1)"), "pm.test('a');pm.expect(1)");
    }

    #[test]
    fn ef_imp_03_code_without_bruno_calls_is_left_alone() {
        let code = "const total = items.reduce((a, b) => a + b, 0);\nconsole.log(total);";
        assert_eq!(translate(code), code);
    }
}
