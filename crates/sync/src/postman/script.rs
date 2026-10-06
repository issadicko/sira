//! Traduction des scripts Postman (`pm.*`) vers l'API `bru` : la table de remplacements de
//! `postman-translations.js` de Bruno, appliquée dans le même ordre, avec pour chaque entrée sa variante `postman.*`.
//!
//! Bruno traduit d'abord par un arbre syntaxique (`postman-to-bruno-translator.js`) et ne retombe sur cette table que
//! si l'analyse échoue ; ici, seule la table est portée. Elle couvre les appels courants (variables, `pm.test`,
//! `pm.expect`, `pm.response.*`, en-têtes, cookies) ; un script plus tordu garde ses `pm.*` non traduits, à relire.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

const REPLACEMENTS: &[(&str, &str)] = &[
    ("pm\\.environment\\.get\\(", "bru.getEnvVar("),
    ("postman\\.environment\\.get\\(", "bru.getEnvVar("),
    ("pm\\.environment\\.set\\(", "bru.setEnvVar("),
    ("postman\\.environment\\.set\\(", "bru.setEnvVar("),
    ("pm\\.variables\\.get\\(", "bru.getVar("),
    ("postman\\.variables\\.get\\(", "bru.getVar("),
    ("pm\\.variables\\.set\\(", "bru.setVar("),
    ("postman\\.variables\\.set\\(", "bru.setVar("),
    ("pm\\.variables\\.replaceIn\\(", "bru.interpolate("),
    ("postman\\.variables\\.replaceIn\\(", "bru.interpolate("),
    ("pm\\.collectionVariables\\.get\\(", "bru.getCollectionVar("),
    ("postman\\.collectionVariables\\.get\\(", "bru.getCollectionVar("),
    ("pm\\.collectionVariables\\.set\\(", "bru.setCollectionVar("),
    ("postman\\.collectionVariables\\.set\\(", "bru.setCollectionVar("),
    ("pm\\.collectionVariables\\.has\\(", "bru.hasCollectionVar("),
    ("postman\\.collectionVariables\\.has\\(", "bru.hasCollectionVar("),
    ("pm\\.collectionVariables\\.unset\\(", "bru.deleteCollectionVar("),
    ("postman\\.collectionVariables\\.unset\\(", "bru.deleteCollectionVar("),
    ("pm\\.collectionVariables\\.clear\\(", "bru.deleteAllCollectionVars("),
    ("postman\\.collectionVariables\\.clear\\(", "bru.deleteAllCollectionVars("),
    ("pm\\.collectionVariables\\.toObject\\(", "bru.getAllCollectionVars("),
    ("postman\\.collectionVariables\\.toObject\\(", "bru.getAllCollectionVars("),
    ("pm\\.setNextRequest\\(null\\)", "bru.runner.stopExecution()"),
    ("postman\\.setNextRequest\\(null\\)", "bru.runner.stopExecution()"),
    ("pm\\.setNextRequest\\(", "bru.runner.setNextRequest("),
    ("postman\\.setNextRequest\\(", "bru.runner.setNextRequest("),
    ("pm\\.test\\(", "test("),
    ("postman\\.test\\(", "test("),
    ("pm.response.to.have\\.status\\(", "expect(res.getStatus()).to.equal("),
    ("pm\\.response\\.to\\.have\\.status\\(", "expect(res.getStatus()).to.equal("),
    ("postman\\.response\\.to\\.have\\.status\\(", "expect(res.getStatus()).to.equal("),
    ("pm\\.response\\.json\\(", "res.getBody("),
    ("postman\\.response\\.json\\(", "res.getBody("),
    ("pm\\.expect\\(", "expect("),
    ("postman\\.expect\\(", "expect("),
    ("pm\\.environment\\.has\\(([^)]+)\\)", "bru.getEnvVar($1) !== undefined && bru.getEnvVar($1) !== null"),
    ("postman\\.environment\\.has\\(([^)]+)\\)", "bru.getEnvVar($1) !== undefined && bru.getEnvVar($1) !== null"),
    ("pm\\.response\\.code", "res.getStatus()"),
    ("postman\\.response\\.code", "res.getStatus()"),
    ("pm\\.response\\.text\\(\\)", "JSON.stringify(res.getBody())"),
    ("postman\\.response\\.text\\(\\)", "JSON.stringify(res.getBody())"),
    ("pm\\.expect\\.fail\\(", "expect.fail("),
    ("postman\\.expect\\.fail\\(", "expect.fail("),
    ("pm\\.response\\.responseTime", "res.getResponseTime()"),
    ("postman\\.response\\.responseTime", "res.getResponseTime()"),
    ("pm\\.globals\\.set\\(", "bru.setGlobalEnvVar("),
    ("postman\\.globals\\.set\\(", "bru.setGlobalEnvVar("),
    ("pm\\.globals\\.get\\(", "bru.getGlobalEnvVar("),
    ("postman\\.globals\\.get\\(", "bru.getGlobalEnvVar("),
    ("pm\\.globals\\.has\\(", "bru.hasGlobalEnvVar("),
    ("postman\\.globals\\.has\\(", "bru.hasGlobalEnvVar("),
    ("pm\\.globals\\.unset\\(", "bru.deleteGlobalEnvVar("),
    ("postman\\.globals\\.unset\\(", "bru.deleteGlobalEnvVar("),
    ("pm\\.globals\\.toObject\\(", "bru.getAllGlobalEnvVars("),
    ("postman\\.globals\\.toObject\\(", "bru.getAllGlobalEnvVars("),
    ("pm\\.globals\\.clear\\(", "bru.deleteAllGlobalEnvVars("),
    ("postman\\.globals\\.clear\\(", "bru.deleteAllGlobalEnvVars("),
    ("pm\\.environment\\.toObject\\(", "bru.getAllEnvVars("),
    ("postman\\.environment\\.toObject\\(", "bru.getAllEnvVars("),
    ("pm\\.environment\\.clear\\(", "bru.deleteAllEnvVars("),
    ("postman\\.environment\\.clear\\(", "bru.deleteAllEnvVars("),
    ("pm\\.variables\\.toObject\\(", "bru.getAllVars("),
    ("postman\\.variables\\.toObject\\(", "bru.getAllVars("),
    ("pm\\.request\\.headers\\.remove\\(", "req.deleteHeader("),
    ("postman\\.request\\.headers\\.remove\\(", "req.deleteHeader("),
    ("pm\\.request\\.headers\\.get\\(", "req.headerList.get("),
    ("postman\\.request\\.headers\\.get\\(", "req.headerList.get("),
    ("pm\\.request\\.headers\\.has\\(", "req.headerList.has("),
    ("postman\\.request\\.headers\\.has\\(", "req.headerList.has("),
    ("pm\\.request\\.headers\\.one\\(", "req.headerList.one("),
    ("postman\\.request\\.headers\\.one\\(", "req.headerList.one("),
    ("pm\\.request\\.headers\\.all\\(", "req.headerList.all("),
    ("postman\\.request\\.headers\\.all\\(", "req.headerList.all("),
    ("pm\\.request\\.headers\\.count\\(", "req.headerList.count("),
    ("postman\\.request\\.headers\\.count\\(", "req.headerList.count("),
    ("pm\\.request\\.headers\\.indexOf\\(", "req.headerList.indexOf("),
    ("postman\\.request\\.headers\\.indexOf\\(", "req.headerList.indexOf("),
    ("pm\\.request\\.headers\\.find\\(", "req.headerList.find("),
    ("postman\\.request\\.headers\\.find\\(", "req.headerList.find("),
    ("pm\\.request\\.headers\\.filter\\(", "req.headerList.filter("),
    ("postman\\.request\\.headers\\.filter\\(", "req.headerList.filter("),
    ("pm\\.request\\.headers\\.each\\(", "req.headerList.each("),
    ("postman\\.request\\.headers\\.each\\(", "req.headerList.each("),
    ("pm\\.request\\.headers\\.map\\(", "req.headerList.map("),
    ("postman\\.request\\.headers\\.map\\(", "req.headerList.map("),
    ("pm\\.request\\.headers\\.reduce\\(", "req.headerList.reduce("),
    ("postman\\.request\\.headers\\.reduce\\(", "req.headerList.reduce("),
    ("pm\\.request\\.headers\\.toObject\\(", "req.headerList.toObject("),
    ("postman\\.request\\.headers\\.toObject\\(", "req.headerList.toObject("),
    ("pm\\.request\\.headers\\.clear\\(", "req.headerList.clear("),
    ("postman\\.request\\.headers\\.clear\\(", "req.headerList.clear("),
    ("pm\\.request\\.headers\\.add\\(", "req.headerList.add("),
    ("postman\\.request\\.headers\\.add\\(", "req.headerList.add("),
    ("pm\\.request\\.headers\\.upsert\\(", "req.headerList.upsert("),
    ("postman\\.request\\.headers\\.upsert\\(", "req.headerList.upsert("),
    ("pm\\.request\\.headers\\.toString\\(", "req.headerList.toString("),
    ("postman\\.request\\.headers\\.toString\\(", "req.headerList.toString("),
    ("pm\\.request\\.headers\\.toJSON\\(", "req.headerList.toJSON("),
    ("postman\\.request\\.headers\\.toJSON\\(", "req.headerList.toJSON("),
    ("pm\\.request\\.headers\\.populate\\(", "req.headerList.populate("),
    ("postman\\.request\\.headers\\.populate\\(", "req.headerList.populate("),
    ("pm\\.request\\.headers\\.repopulate\\(", "req.headerList.repopulate("),
    ("postman\\.request\\.headers\\.repopulate\\(", "req.headerList.repopulate("),
    ("pm\\.request\\.headers\\.assimilate\\(", "req.headerList.assimilate("),
    ("postman\\.request\\.headers\\.assimilate\\(", "req.headerList.assimilate("),
    ("pm\\.request\\.headers\\.prepend\\(", "req.headerList.add("),
    ("postman\\.request\\.headers\\.prepend\\(", "req.headerList.add("),
    ("pm\\.request\\.headers\\.insert\\(", "req.headerList.add("),
    ("postman\\.request\\.headers\\.insert\\(", "req.headerList.add("),
    ("pm\\.request\\.headers\\.insertAfter\\(", "req.headerList.add("),
    ("postman\\.request\\.headers\\.insertAfter\\(", "req.headerList.add("),
    ("pm\\.response\\.headers\\.get\\(", "res.getHeader("),
    ("postman\\.response\\.headers\\.get\\(", "res.getHeader("),
    ("pm\\.response\\.headers\\.has\\(", "res.headerList.has("),
    ("postman\\.response\\.headers\\.has\\(", "res.headerList.has("),
    ("pm\\.response\\.headers\\.one\\(", "res.headerList.one("),
    ("postman\\.response\\.headers\\.one\\(", "res.headerList.one("),
    ("pm\\.response\\.headers\\.all\\(", "res.headerList.all("),
    ("postman\\.response\\.headers\\.all\\(", "res.headerList.all("),
    ("pm\\.response\\.headers\\.count\\(", "res.headerList.count("),
    ("postman\\.response\\.headers\\.count\\(", "res.headerList.count("),
    ("pm\\.response\\.headers\\.indexOf\\(", "res.headerList.indexOf("),
    ("postman\\.response\\.headers\\.indexOf\\(", "res.headerList.indexOf("),
    ("pm\\.response\\.headers\\.find\\(", "res.headerList.find("),
    ("postman\\.response\\.headers\\.find\\(", "res.headerList.find("),
    ("pm\\.response\\.headers\\.filter\\(", "res.headerList.filter("),
    ("postman\\.response\\.headers\\.filter\\(", "res.headerList.filter("),
    ("pm\\.response\\.headers\\.each\\(", "res.headerList.each("),
    ("postman\\.response\\.headers\\.each\\(", "res.headerList.each("),
    ("pm\\.response\\.headers\\.map\\(", "res.headerList.map("),
    ("postman\\.response\\.headers\\.map\\(", "res.headerList.map("),
    ("pm\\.response\\.headers\\.reduce\\(", "res.headerList.reduce("),
    ("postman\\.response\\.headers\\.reduce\\(", "res.headerList.reduce("),
    ("pm\\.response\\.headers\\.toObject\\(", "res.headerList.toObject("),
    ("postman\\.response\\.headers\\.toObject\\(", "res.headerList.toObject("),
    ("pm\\.response\\.headers\\.toString\\(", "res.headerList.toString("),
    ("postman\\.response\\.headers\\.toString\\(", "res.headerList.toString("),
    ("pm\\.response\\.headers\\.toJSON\\(", "res.headerList.toJSON("),
    ("postman\\.response\\.headers\\.toJSON\\(", "res.headerList.toJSON("),
    ("pm\\.response\\.to\\.have\\.jsonSchema\\(", "expect(res.getBody()).to.have.jsonSchema("),
    ("postman\\.response\\.to\\.have\\.jsonSchema\\(", "expect(res.getBody()).to.have.jsonSchema("),
    ("pm\\.response\\.to\\.not\\.have\\.jsonSchema\\(", "expect(res.getBody()).to.not.have.jsonSchema("),
    ("postman\\.response\\.to\\.not\\.have\\.jsonSchema\\(", "expect(res.getBody()).to.not.have.jsonSchema("),
    ("pm\\.response\\.not\\.to\\.have\\.jsonSchema\\(", "expect(res.getBody()).not.to.have.jsonSchema("),
    ("postman\\.response\\.not\\.to\\.have\\.jsonSchema\\(", "expect(res.getBody()).not.to.have.jsonSchema("),
    ("pm\\.response\\.to\\.have\\.not\\.jsonSchema\\(", "expect(res.getBody()).to.have.not.jsonSchema("),
    ("postman\\.response\\.to\\.have\\.not\\.jsonSchema\\(", "expect(res.getBody()).to.have.not.jsonSchema("),
    ("pm\\.response\\.to\\.have\\.jsonBody\\(", "expect(res.getBody()).to.have.jsonBody("),
    ("postman\\.response\\.to\\.have\\.jsonBody\\(", "expect(res.getBody()).to.have.jsonBody("),
    ("pm\\.response\\.to\\.not\\.have\\.jsonBody\\(", "expect(res.getBody()).to.not.have.jsonBody("),
    ("postman\\.response\\.to\\.not\\.have\\.jsonBody\\(", "expect(res.getBody()).to.not.have.jsonBody("),
    ("pm\\.response\\.not\\.to\\.have\\.jsonBody\\(", "expect(res.getBody()).not.to.have.jsonBody("),
    ("postman\\.response\\.not\\.to\\.have\\.jsonBody\\(", "expect(res.getBody()).not.to.have.jsonBody("),
    ("pm\\.response\\.to\\.have\\.not\\.jsonBody\\(", "expect(res.getBody()).to.have.not.jsonBody("),
    ("postman\\.response\\.to\\.have\\.not\\.jsonBody\\(", "expect(res.getBody()).to.have.not.jsonBody("),
    ("pm\\.response\\.to\\.have\\.body\\(", "expect(res.getBody()).to.equal("),
    ("postman\\.response\\.to\\.have\\.body\\(", "expect(res.getBody()).to.equal("),
    ("pm\\.response\\.to\\.have\\.header\\(", "expect(res.getHeaders()).to.have.property("),
    ("postman\\.response\\.to\\.have\\.header\\(", "expect(res.getHeaders()).to.have.property("),
    ("pm\\.response\\.size\\(\\)", "res.getSize()"),
    ("postman\\.response\\.size\\(\\)", "res.getSize()"),
    ("pm\\.response\\.size\\(\\)\\.body", "res.getSize().body"),
    ("postman\\.response\\.size\\(\\)\\.body", "res.getSize().body"),
    ("pm\\.response\\.responseSize", "res.getSize().body"),
    ("postman\\.response\\.responseSize", "res.getSize().body"),
    ("pm\\.response\\.size\\(\\)\\.header", "res.getSize().header"),
    ("postman\\.response\\.size\\(\\)\\.header", "res.getSize().header"),
    ("pm\\.response\\.size\\(\\)\\.total", "res.getSize().total"),
    ("postman\\.response\\.size\\(\\)\\.total", "res.getSize().total"),
    ("pm\\.environment\\.name", "bru.getEnvName()"),
    ("postman\\.environment\\.name", "bru.getEnvName()"),
    ("pm\\.response\\.status", "res.statusText"),
    ("postman\\.response\\.status", "res.statusText"),
    ("pm\\.response\\.headers", "res.getHeaders()"),
    ("postman\\.response\\.headers", "res.getHeaders()"),
    ("tests\\['([^']+)'\\]\\s*=\\s*([^;]+);", "test(\"$1\", function() { expect(Boolean($2)).to.be.true; });"),
    ("pm\\.request\\.url\\.getHost\\(\\)", "req.getHost()"),
    ("postman\\.request\\.url\\.getHost\\(\\)", "req.getHost()"),
    ("pm\\.request\\.url\\.getPath\\(\\)", "req.getPath()"),
    ("postman\\.request\\.url\\.getPath\\(\\)", "req.getPath()"),
    ("pm\\.request\\.url\\.getQueryString\\(\\)", "req.getQueryString()"),
    ("postman\\.request\\.url\\.getQueryString\\(\\)", "req.getQueryString()"),
    ("pm\\.request\\.url\\.variables", "req.getPathParams()"),
    ("postman\\.request\\.url\\.variables", "req.getPathParams()"),
    ("pm\\.request\\.url", "req.getUrl()"),
    ("postman\\.request\\.url", "req.getUrl()"),
    ("pm\\.request\\.method", "req.getMethod()"),
    ("postman\\.request\\.method", "req.getMethod()"),
    ("pm\\.request\\.headers", "req.getHeaders()"),
    ("postman\\.request\\.headers", "req.getHeaders()"),
    ("pm\\.request\\.body", "req.getBody()"),
    ("postman\\.request\\.body", "req.getBody()"),
    ("pm\\.info\\.requestName", "req.getName()"),
    ("postman\\.info\\.requestName", "req.getName()"),
    ("request\\.url", "req.getUrl()"),
    ("request\\.method", "req.getMethod()"),
    ("request\\.headers", "req.getHeaders()"),
    ("request\\.body", "req.getBody()"),
    ("request\\.name", "req.getName()"),
    ("postman\\.setEnvironmentVariable\\(", "bru.setEnvVar("),
    ("postman\\.getEnvironmentVariable\\(", "bru.getEnvVar("),
    ("postman\\.clearEnvironmentVariable\\(", "bru.deleteEnvVar("),
    ("pm\\.execution\\.skipRequest\\(\\)", "bru.runner.skipRequest()"),
    ("postman\\.execution\\.skipRequest\\(\\)", "bru.runner.skipRequest()"),
    ("pm\\.execution\\.skipRequest", "bru.runner.skipRequest"),
    ("postman\\.execution\\.skipRequest", "bru.runner.skipRequest"),
    ("pm\\.execution\\.setNextRequest\\(null\\)", "bru.runner.stopExecution()"),
    ("postman\\.execution\\.setNextRequest\\(null\\)", "bru.runner.stopExecution()"),
    ("pm\\.execution\\.setNextRequest\\(", "bru.runner.setNextRequest("),
    ("postman\\.execution\\.setNextRequest\\(", "bru.runner.setNextRequest("),
    ("pm\\.cookies\\.jar\\(\\)\\.get\\(", "bru.cookies.jar().getCookie("),
    ("postman\\.cookies\\.jar\\(\\)\\.get\\(", "bru.cookies.jar().getCookie("),
    ("pm\\.cookies\\.jar\\(\\)\\.set\\(", "bru.cookies.jar().setCookie("),
    ("postman\\.cookies\\.jar\\(\\)\\.set\\(", "bru.cookies.jar().setCookie("),
    ("pm\\.cookies\\.jar\\(\\)\\.unset\\(", "bru.cookies.jar().deleteCookie("),
    ("postman\\.cookies\\.jar\\(\\)\\.unset\\(", "bru.cookies.jar().deleteCookie("),
    ("pm\\.cookies\\.jar\\(\\)\\.clear\\(", "bru.cookies.jar().deleteCookies("),
    ("postman\\.cookies\\.jar\\(\\)\\.clear\\(", "bru.cookies.jar().deleteCookies("),
    ("pm\\.cookies\\.jar\\(\\)\\.getAll\\(", "bru.cookies.jar().getCookies("),
    ("postman\\.cookies\\.jar\\(\\)\\.getAll\\(", "bru.cookies.jar().getCookies("),
    ("pm\\.cookies\\.jar\\(\\)", "bru.cookies.jar()"),
    ("postman\\.cookies\\.jar\\(\\)", "bru.cookies.jar()"),
    ("pm\\.cookies\\.get\\(", "bru.cookies.get("),
    ("postman\\.cookies\\.get\\(", "bru.cookies.get("),
    ("pm\\.cookies\\.has\\(", "bru.cookies.has("),
    ("postman\\.cookies\\.has\\(", "bru.cookies.has("),
    ("pm\\.cookies\\.toObject\\(", "bru.cookies.toObject("),
    ("postman\\.cookies\\.toObject\\(", "bru.cookies.toObject("),
    ("pm\\.cookies\\.toString\\(", "bru.cookies.toString("),
    ("postman\\.cookies\\.toString\\(", "bru.cookies.toString("),
    ("pm\\.cookies\\.clear\\(", "bru.cookies.clear("),
    ("postman\\.cookies\\.clear\\(", "bru.cookies.clear("),
    ("pm\\.cookies\\.remove\\(", "bru.cookies.delete("),
    ("postman\\.cookies\\.remove\\(", "bru.cookies.delete("),
    ("pm\\.cookies\\.one\\(", "bru.cookies.one("),
    ("postman\\.cookies\\.one\\(", "bru.cookies.one("),
    ("pm\\.cookies\\.all\\(", "bru.cookies.all("),
    ("postman\\.cookies\\.all\\(", "bru.cookies.all("),
    ("pm\\.cookies\\.idx\\(", "bru.cookies.idx("),
    ("postman\\.cookies\\.idx\\(", "bru.cookies.idx("),
    ("pm\\.cookies\\.count\\(", "bru.cookies.count("),
    ("postman\\.cookies\\.count\\(", "bru.cookies.count("),
    ("pm\\.cookies\\.indexOf\\(", "bru.cookies.indexOf("),
    ("postman\\.cookies\\.indexOf\\(", "bru.cookies.indexOf("),
    ("pm\\.cookies\\.find\\(", "bru.cookies.find("),
    ("postman\\.cookies\\.find\\(", "bru.cookies.find("),
    ("pm\\.cookies\\.filter\\(", "bru.cookies.filter("),
    ("postman\\.cookies\\.filter\\(", "bru.cookies.filter("),
    ("pm\\.cookies\\.each\\(", "bru.cookies.each("),
    ("postman\\.cookies\\.each\\(", "bru.cookies.each("),
    ("pm\\.cookies\\.map\\(", "bru.cookies.map("),
    ("postman\\.cookies\\.map\\(", "bru.cookies.map("),
    ("pm\\.cookies\\.reduce\\(", "bru.cookies.reduce("),
    ("postman\\.cookies\\.reduce\\(", "bru.cookies.reduce("),
    ("pm\\.cookies\\.add\\(", "bru.cookies.add("),
    ("postman\\.cookies\\.add\\(", "bru.cookies.add("),
    ("pm\\.cookies\\.upsert\\(", "bru.cookies.upsert("),
    ("postman\\.cookies\\.upsert\\(", "bru.cookies.upsert("),
    ("pm\\.cookies\\.prepend\\(", "bru.cookies.add("),
    ("postman\\.cookies\\.prepend\\(", "bru.cookies.add("),
    ("pm\\.cookies\\.insert\\(", "bru.cookies.add("),
    ("postman\\.cookies\\.insert\\(", "bru.cookies.add("),
    ("pm\\.cookies\\.insertAfter\\(", "bru.cookies.add("),
    ("postman\\.cookies\\.insertAfter\\(", "bru.cookies.add("),
];

fn compiled() -> &'static [(Regex, &'static str)] {
    static TABLE: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        REPLACEMENTS
            .iter()
            .filter_map(|(pattern, replacement)| Regex::new(pattern).ok().map(|regex| (regex, *replacement)))
            .collect()
    })
}

/// Le code d'un événement Postman : les lignes de `exec` jointes, puis traduites.
pub fn translate(exec: &Value) -> String {
    let code = match exec {
        Value::Array(lines) => {
            lines.iter().map(|l| l.as_str().map_or_else(|| l.to_string(), str::to_owned)).collect::<Vec<_>>().join("\n")
        }
        Value::String(code) => code.clone(),
        other => other.to_string(),
    };
    compiled().iter().fold(code, |code, (regex, replacement)| regex.replace_all(&code, *replacement).into_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn ef_imp_01_every_replacement_of_the_table_compiles() {
        assert_eq!(compiled().len(), REPLACEMENTS.len());
    }

    #[test]
    fn ef_imp_01_variables_tests_and_responses_become_their_bru_equivalents() {
        let code = json!([
            "pm.environment.set('token', pm.response.json().token);",
            "pm.test('ok', function () { pm.expect(pm.response.code).to.equal(200); });",
            "const v = pm.variables.get('a') + pm.collectionVariables.get('b');",
        ]);
        assert_eq!(
            translate(&code),
            "bru.setEnvVar('token', res.getBody().token);\ntest('ok', function () { expect(res.getStatus()).to.equal(200); });\nconst v = bru.getVar('a') + bru.getCollectionVar('b');"
        );
    }

    #[test]
    fn ef_imp_01_the_legacy_postman_object_and_the_tests_dictionary_are_translated() {
        assert_eq!(translate(&json!("postman.setEnvironmentVariable('k', 'v');")), "bru.setEnvVar('k', 'v');");
        assert_eq!(
            translate(&json!("tests['Status is 200'] = responseCode.code === 200;")),
            "test(\"Status is 200\", function() { expect(Boolean(responseCode.code === 200)).to.be.true; });"
        );
    }

    #[test]
    fn ef_imp_01_a_null_next_request_stops_the_run_and_a_name_jumps() {
        assert_eq!(translate(&json!("pm.setNextRequest(null);")), "bru.runner.stopExecution();");
        assert_eq!(translate(&json!("pm.setNextRequest('Login');")), "bru.runner.setNextRequest('Login');");
    }

    #[test]
    fn ef_imp_01_environment_has_and_request_accessors_are_rewritten() {
        assert_eq!(
            translate(&json!("if (pm.environment.has('x')) {}")),
            "if (bru.getEnvVar('x') !== undefined && bru.getEnvVar('x') !== null) {}"
        );
        assert_eq!(translate(&json!("pm.request.url + pm.request.method")), "req.getUrl() + req.getMethod()");
    }

    #[test]
    fn ef_imp_01_code_without_postman_calls_is_left_alone() {
        let code = "const total = items.reduce((a, b) => a + b, 0);\nconsole.log(total);";
        assert_eq!(translate(&json!(code)), code);
    }
}
