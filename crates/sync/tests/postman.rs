use std::fs;
use std::path::Path;

use xc_core::collection::TreeItem;
use xc_core::{open_collection, read_environment, read_request, Auth, Body, MultipartValue, ParamKind};
use xc_sync::import::{import_postman, import_postman_environment};
use xc_sync::manage::create_environment;

const SHOP: &str = include_str!("fixtures/postman-shop.json");
const ENV: &str = include_str!("fixtures/postman-env.json");

fn imported() -> (tempfile::TempDir, std::path::PathBuf, Vec<xc_sync::postman::Issue>) {
    let dir = tempfile::tempdir().unwrap();
    let (root, issues) = import_postman(SHOP, dir.path()).unwrap();
    (dir, root, issues)
}

fn paths(items: &[TreeItem], out: &mut Vec<String>) {
    for item in items {
        out.push(item.path().to_owned());
        if let TreeItem::Folder { children, .. } = item {
            paths(children, out);
        }
    }
}

fn read(root: &Path, path: &str) -> xc_core::RequestDoc {
    read_request(root, path).unwrap_or_else(|e| panic!("{path} : {e}"))
}

#[test]
fn ef_imp_01_a_postman_collection_becomes_a_bruno_compatible_folder_tree() {
    let (_dir, root, _) = imported();
    let info = open_collection(&root).unwrap();
    assert_eq!(info.name, "Shop API");
    assert_eq!(root.file_name().unwrap(), "Shop API");
    let mut all = Vec::new();
    paths(&info.items, &mut all);
    for expected in [
        "Users",
        "Users/List users.yml",
        "Users/Create user.yml",
        "Users/Upload avatar.yml",
        "Login.yml",
        "Login_1.yml",
        "Products query.yml",
        "Token.yml",
    ] {
        assert!(all.iter().any(|p| p == expected), "{expected} absent de {all:?}");
    }
    assert!(!all.iter().any(|p| p.contains("Broken")), "la requête sans méthode est écartée : {all:?}");
    assert_eq!(info.request_count, 7);
}

#[test]
fn ef_imp_01_issues_name_the_items_that_could_not_be_converted() {
    let (_dir, _, issues) = imported();
    let paths: Vec<_> = issues.iter().map(|i| (i.path.as_str(), i.severity)).collect();
    assert!(paths.contains(&("Broken", "error")), "{paths:?}");
    assert!(paths.iter().any(|(p, s)| p.starts_with("Item ") && *s == "error"), "{paths:?}");
    assert_eq!(issues.len(), 2, "{issues:?}");
}

#[test]
fn ef_imp_01_a_request_keeps_its_method_url_params_headers_and_post_response_script() {
    let (_dir, root, _) = imported();
    let list = read(&root, "Users/List users.yml");
    assert_eq!((list.method.as_str(), list.url.as_str()), ("GET", "{{baseUrl}}/users/:team?page=1&verbose=true"));
    let query: Vec<_> = list
        .params
        .iter()
        .filter(|p| p.kind == ParamKind::Query)
        .map(|p| (p.name.as_str(), p.value.as_str(), p.enabled))
        .collect();
    assert_eq!(query, [("page", "1", true), ("verbose", "true", false)]);
    let path: Vec<_> =
        list.params.iter().filter(|p| p.kind == ParamKind::Path).map(|p| (p.name.as_str(), p.value.as_str())).collect();
    assert_eq!(path, [("team", "core")]);
    let headers: Vec<_> = list.headers.iter().map(|h| (h.name.as_str(), h.enabled)).collect();
    assert_eq!(headers, [("Accept", true), ("X-Debug", false)]);
    assert!(matches!(list.auth, Auth::Inherit), "{:?}", list.auth);

    let after = list.scripts.iter().find(|s| s.kind == "after-response").expect("script post-réponse");
    assert!(after.code.contains("test('status is 200'"), "{}", after.code);
    assert!(
        after.code.contains("expect(res.getStatus()).to.equal(200)") || after.code.contains("res.getStatus()"),
        "{}",
        after.code
    );
    assert!(after.code.contains("bru.setEnvVar('firstUser', res.getBody()[0].id)"), "{}", after.code);
}

#[test]
fn ef_imp_01_bodies_of_every_mode_are_converted() {
    let (_dir, root, _) = imported();
    let create = read(&root, "Users/Create user.yml");
    assert!(matches!(&create.body, Body::Json { data } if data == "{\"name\":\"Aminata\"}"), "{:?}", create.body);
    assert_eq!(create.docs.as_deref(), Some("Crée un compte"));

    let upload = read(&root, "Users/Upload avatar.yml");
    let Body::MultipartForm { fields } = &upload.body else { panic!("{:?}", upload.body) };
    assert_eq!(fields[0].value, MultipartValue::Text("Photo".into()));
    assert_eq!(fields[1].value, MultipartValue::File(vec!["avatars/aminata.png".into()]));
    assert_eq!(fields[1].content_type.as_deref(), Some("image/png"));

    let login = read(&root, "Login.yml");
    let Body::FormUrlEncoded { fields } = &login.body else { panic!("{:?}", login.body) };
    assert_eq!(
        fields.iter().map(|f| (f.name.as_str(), f.enabled)).collect::<Vec<_>>(),
        [("grant", true), ("user", false)]
    );
    assert_eq!(login.url, "{{baseUrl}}/login", "le fragment de l'adresse est retiré");
}

#[test]
fn ef_imp_01_an_url_without_raw_is_rebuilt_from_its_parts() {
    let (_dir, root, _) = imported();
    assert_eq!(read(&root, "Login_1.yml").url, "https://shop.test/logout?all=1");
}

#[test]
fn ef_imp_01_graphql_requests_keep_their_query_and_variables() {
    let (_dir, root, _) = imported();
    let text = fs::read_to_string(root.join("Products query.yml")).unwrap();
    assert!(text.contains("type: graphql"), "{text}");
    assert!(
        text.contains("query: '{ products { id } }'")
            || text.contains("query: \"{ products { id } }\"")
            || text.contains("{ products { id } }"),
        "{text}"
    );
    assert!(text.contains("first"), "{text}");
}

#[test]
fn ef_imp_01_auth_is_inherited_by_default_and_carried_at_every_level() {
    let (_dir, root, _) = imported();
    let collection = fs::read_to_string(root.join("opencollection.yml")).unwrap();
    assert!(collection.contains("type: bearer") && collection.contains("{{token}}"), "{collection}");
    let folder = fs::read_to_string(root.join("Users/folder.yml")).unwrap();
    assert!(folder.contains("type: basic") && folder.contains("username: admin"), "{folder}");

    let create = read(&root, "Users/Create user.yml");
    assert!(
        matches!(&create.auth, Auth::Apikey { key, placement, .. } if key == "api_key" && placement == "query"),
        "{:?}",
        create.auth
    );
    assert!(matches!(read(&root, "Users/Upload avatar.yml").auth, Auth::None));

    let Auth::Oauth2(oauth) = read(&root, "Token.yml").auth else { panic!("OAuth 2 attendu") };
    assert_eq!(oauth.flow, "client_credentials");
    assert_eq!(
        (oauth.client_id.as_str(), oauth.token_id.as_str(), oauth.credentials_placement.as_str()),
        ("shop", "shopToken", "body")
    );
    assert_eq!(oauth.parameters.len(), 1);
    assert_eq!(
        (oauth.parameters[0].name.as_str(), oauth.parameters[0].stage.as_str(), oauth.parameters[0].placement.as_str()),
        ("audience", "token", "body")
    );
}

#[test]
fn ef_imp_01_collection_variables_scripts_and_docs_land_in_opencollection_yml() {
    let (_dir, root, _) = imported();
    let text = fs::read_to_string(root.join("opencollection.yml")).unwrap();
    assert!(text.contains("baseUrl") && text.contains("https://shop.test/v1"), "{text}");
    assert!(text.contains("page_size"), "un nom de variable invalide est assaini : {text}");
    assert!(text.contains("bru.setVar('startedAt'"), "{text}");
    assert!(text.contains("API de démonstration"), "{text}");
}

#[test]
fn ef_imp_01_saved_responses_become_examples() {
    let (_dir, root, _) = imported();
    let text = fs::read_to_string(root.join("Users/List users.yml")).unwrap();
    assert!(text.contains("examples:") && text.contains("name: Two users") && text.contains("status: 200"), "{text}");
}

#[test]
fn ef_imp_01_the_imported_files_are_read_back_without_loss_by_the_editor() {
    let (_dir, root, _) = imported();
    for path in ["Users/List users.yml", "Users/Create user.yml", "Login.yml", "Token.yml"] {
        let doc = read(&root, path);
        let text = fs::read_to_string(root.join(path)).unwrap();
        xc_core::save_request(&root, path, &doc).unwrap();
        assert_eq!(fs::read_to_string(root.join(path)).unwrap(), text, "{path} ne revient pas à l'identique");
    }
}

#[test]
fn ef_imp_01_two_imports_of_the_same_collection_never_overwrite_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = import_postman(SHOP, dir.path()).unwrap();
    let (second, _) = import_postman(SHOP, dir.path()).unwrap();
    assert_ne!(first, second);
    assert!(first.is_dir() && second.is_dir());
}

#[test]
fn ef_imp_01_an_enveloped_export_and_other_schemas_are_handled() {
    let dir = tempfile::tempdir().unwrap();
    let wrapped = format!("{{\"collection\": {SHOP}}}");
    assert!(import_postman(&wrapped, dir.path()).is_ok(), "les exports récents enveloppent la collection");
    let old = SHOP.replace("v2.1.0", "v1.0.0");
    let refused = import_postman(&old, dir.path()).unwrap_err();
    assert!(refused.to_string().contains("v2.0 et v2.1"), "{refused}");
    assert!(refused.is_input());
    assert!(import_postman("pas du json", dir.path()).unwrap_err().is_input());
}

#[test]
fn ef_imp_01_a_postman_environment_is_added_to_a_collection_without_replacing_another() {
    let (_dir, root, _) = imported();
    create_environment(&root, "Prod").unwrap();
    let name = import_postman_environment(ENV, &root).unwrap();
    assert_eq!(name, "Prod 1", "l'environnement existant n'est pas remplacé");
    let vars = read_environment(&root, &name).unwrap();
    let list: Vec<_> = vars.iter().map(|v| (v.name.as_str(), v.value.as_deref(), v.secret, v.enabled)).collect();
    assert_eq!(
        list,
        [
            ("base-url", Some("https://shop.test"), false, true),
            ("api_key", None, true, true),
            ("off", Some("x"), false, false)
        ]
    );
}
