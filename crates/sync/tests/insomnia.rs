use std::fs;
use std::path::Path;

use xc_core::collection::TreeItem;
use xc_core::{open_collection, read_environment, read_request, Auth, Body, MultipartValue, ParamKind};
use xc_sync::import::import_insomnia;

const V4: &str = include_str!("fixtures/insomnia-v4.json");
const V5: &str = include_str!("fixtures/insomnia-v5.yaml");

fn imported(text: &str) -> (tempfile::TempDir, std::path::PathBuf, Vec<xc_sync::postman::Issue>) {
    let dir = tempfile::tempdir().unwrap();
    let (root, issues) = import_insomnia(text, dir.path()).unwrap();
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

fn tree(root: &Path) -> Vec<String> {
    let mut all = Vec::new();
    paths(&open_collection(root).unwrap().items, &mut all);
    all
}

fn read(root: &Path, path: &str) -> xc_core::RequestDoc {
    read_request(root, path).unwrap_or_else(|e| panic!("{path} : {e}"))
}

#[test]
fn ef_imp_02_an_insomnia_v4_export_becomes_a_folder_tree_without_duplicated_requests() {
    let (_dir, root, _) = imported(V4);
    let info = open_collection(&root).unwrap();
    assert_eq!(info.name, "Shop Insomnia");
    let all = tree(&root);
    for expected in [
        "Users",
        "Users/Admin",
        "Users/Admin/Ban.yml",
        "Users/List users.yml",
        "Users/Create user.yml",
        "Login.yml",
        "Login_1.yml",
        "Soap.yml",
        "Products.yml",
        "Token.yml",
    ] {
        assert!(all.iter().any(|p| p == expected), "{expected} absent de {all:?}");
    }
    assert_eq!(all.iter().filter(|p| p.ends_with("Ban.yml")).count(), 1, "{all:?}");
    assert_eq!(info.request_count, 10);
}

#[test]
fn ef_imp_02_items_keep_the_order_insomnia_shows() {
    let (_dir, root, _) = imported(V4);
    let list = read(&root, "Users/List users.yml");
    let create = read(&root, "Users/Create user.yml");
    assert!(list.seq < create.seq, "{:?} {:?}", list.seq, create.seq);
    let login = read(&root, "Login.yml");
    let soap = read(&root, "Soap.yml");
    assert!(login.seq < soap.seq);
}

#[test]
fn ef_imp_02_insomnia_variables_lose_their_prefix_and_spaces() {
    let (_dir, root, _) = imported(V4);
    let list = read(&root, "Users/List users.yml");
    assert_eq!(list.url, "{{base_url}}/users/:team?page=1");
    let headers: Vec<_> = list.headers.iter().map(|h| (h.name.as_str(), h.value.as_str(), h.enabled)).collect();
    assert_eq!(headers, [("Accept", "application/json", true), ("X-Debug", "{{debug}}", false)]);
    let path: Vec<_> =
        list.params.iter().filter(|p| p.kind == ParamKind::Path).map(|p| (p.name.as_str(), p.value.as_str())).collect();
    assert_eq!(path, [("team", "core")]);
    let query: Vec<_> = list
        .params
        .iter()
        .filter(|p| p.kind == ParamKind::Query)
        .map(|p| (p.name.as_str(), p.value.as_str(), p.enabled))
        .collect();
    assert_eq!(query, [("verbose", "true", false)]);
}

#[test]
fn ef_imp_02_bodies_of_every_mode_are_converted() {
    let (_dir, root, _) = imported(V4);
    let create = read(&root, "Users/Create user.yml");
    assert!(matches!(&create.body, Body::Json { data } if data == "{\"name\": \"{{who}}\"}"), "{:?}", create.body);

    let login = read(&root, "Login.yml");
    let Body::FormUrlEncoded { fields } = &login.body else { panic!("{:?}", login.body) };
    assert_eq!(
        fields.iter().map(|f| (f.name.as_str(), f.enabled)).collect::<Vec<_>>(),
        [("grant", true), ("user", false)]
    );

    let Body::MultipartForm { fields } = read(&root, "Login_1.yml").body else { panic!("multipart attendu") };
    assert_eq!(fields[0].value, MultipartValue::Text("Photo".into()));

    assert!(matches!(&read(&root, "Soap.yml").body, Body::Xml { data } if data == "<a/>"));

    let graphql = fs::read_to_string(root.join("Products.yml")).unwrap();
    assert!(
        graphql.contains("type: graphql") && graphql.contains("{ products { id } }") && graphql.contains("first"),
        "{graphql}"
    );
}

#[test]
fn ef_imp_02_authentication_is_carried_for_every_supported_type() {
    let (_dir, root, issues) = imported(V4);
    assert!(matches!(&read(&root, "Users/List users.yml").auth, Auth::Bearer { token } if token == "{{token}}"));
    assert!(
        matches!(&read(&root, "Users/Create user.yml").auth, Auth::Basic { username, password } if username == "admin" && password == "{{pwd}}")
    );
    assert!(matches!(&read(&root, "Login_1.yml").auth, Auth::Digest { .. }), "{:?}", read(&root, "Login_1.yml").auth);
    assert!(
        matches!(&read(&root, "Soap.yml").auth, Auth::Apikey { key, placement, .. } if key == "api_key" && placement == "query")
    );
    let Auth::Oauth2(oauth) = read(&root, "Token.yml").auth else { panic!("OAuth 2 attendu") };
    assert_eq!(oauth.flow, "client_credentials");
    assert_eq!((oauth.client_id.as_str(), oauth.credentials_placement.as_str()), ("shop", "body"));
    assert!(
        matches!(read(&root, "Off.yml").auth, Auth::None),
        "une auth désactivée dans Insomnia ne doit pas être envoyée"
    );

    let unsupported: Vec<_> = issues.iter().map(|i| (i.path.as_str(), i.severity)).collect();
    assert_eq!(unsupported, [("Hawk", "warning")], "{issues:?}");
    assert!(matches!(read(&root, "Hawk.yml").auth, Auth::None));
}

#[test]
fn ef_imp_02_encode_url_follows_the_request_setting() {
    let (_dir, root, _) = imported(V4);
    let text = fs::read_to_string(root.join("Users/List users.yml")).unwrap();
    assert!(text.contains("encodeUrl: false"), "{text}");
    let other = fs::read_to_string(root.join("Users/Create user.yml")).unwrap();
    assert!(!other.contains("encodeUrl: false"), "{other}");
}

#[test]
fn ef_imp_02_v4_environments_are_flattened_and_sub_environments_inherit_the_base() {
    let (_dir, root, _) = imported(V4);
    let base = read_environment(&root, "Base Environment").unwrap();
    let list: Vec<_> = base.iter().map(|v| (v.name.as_str(), v.value.as_deref().unwrap_or_default())).collect();
    assert_eq!(
        list,
        [
            ("base_url", "https://shop.test"),
            ("debug", "false"),
            ("nested.a", "1"),
            ("nested.list[0]", "x"),
            ("nested.list[1]", "y")
        ]
    );
    let prod = read_environment(&root, "Prod").unwrap();
    let value = |name: &str| prod.iter().find(|v| v.name == name).and_then(|v| v.value.clone());
    assert_eq!(value("base_url").as_deref(), Some("https://shop.prod"));
    assert_eq!(value("nested.a").as_deref(), Some("1"), "la base est héritée");
    assert_eq!(value("extra").as_deref(), Some("e"));
    assert!(read_environment(&root, "Environment 3").is_ok(), "un environnement sans nom est numéroté");
}

#[test]
fn ef_imp_02_an_insomnia_v5_yaml_export_is_converted() {
    let (_dir, root, issues) = imported(V5);
    let info = open_collection(&root).unwrap();
    assert_eq!(info.name, "Shop v5");
    let all = tree(&root);
    for expected in ["Users", "Users/List users.yml", "Users/List users_1.yml", "Create.yml", "Plain.yml"] {
        assert!(all.iter().any(|p| p == expected), "{expected} absent de {all:?}");
    }
    assert_eq!(info.request_count, 4);

    assert_eq!(read(&root, "Users/List users.yml").url, "{{base_url}}/users");
    assert!(matches!(&read(&root, "Users/List users.yml").auth, Auth::Bearer { token } if token == "{{token}}"));
    let again = fs::read_to_string(root.join("Users/List users_1.yml")).unwrap();
    assert!(again.contains("encodeUrl: false"), "{again}");
    assert!(matches!(&read(&root, "Create.yml").body, Body::Json { data } if data == "{\"name\": \"x\"}"));
    assert!(matches!(&read(&root, "Plain.yml").body, Body::Text { data } if data == "hello"));

    let skipped: Vec<_> = issues.iter().map(|i| (i.path.as_str(), i.severity)).collect();
    assert_eq!(skipped, [("Note", "error")], "{issues:?}");
}

#[test]
fn ef_imp_02_v5_environments_merge_each_sub_environment_over_the_base() {
    let (_dir, root, _) = imported(V5);
    let base = read_environment(&root, "Base Environment").unwrap();
    assert_eq!(
        base.iter().map(|v| (v.name.as_str(), v.value.as_deref().unwrap_or_default())).collect::<Vec<_>>(),
        [("base_url", "https://shop.test"), ("retries", "3")]
    );
    let staging = read_environment(&root, "Staging").unwrap();
    assert_eq!(
        staging.iter().map(|v| (v.name.as_str(), v.value.as_deref().unwrap_or_default())).collect::<Vec<_>>(),
        [("base_url", "https://shop.staging"), ("retries", "3")]
    );
}

#[test]
fn ef_imp_02_the_imported_files_are_read_back_without_loss_by_the_editor() {
    let (_dir, root, _) = imported(V4);
    for path in ["Users/List users.yml", "Users/Create user.yml", "Login.yml", "Token.yml", "Soap.yml"] {
        let doc = read(&root, path);
        let text = fs::read_to_string(root.join(path)).unwrap();
        xc_core::save_request(&root, path, &doc).unwrap();
        assert_eq!(fs::read_to_string(root.join(path)).unwrap(), text, "{path} ne revient pas à l'identique");
    }
}

#[test]
fn ef_imp_02_bad_input_is_reported_as_input_not_as_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let no_workspace = import_insomnia(r#"{"_type":"export","resources":[]}"#, dir.path()).unwrap_err();
    assert!(no_workspace.is_input(), "{no_workspace}");
    assert!(no_workspace.to_string().contains("workspace"));
    assert!(import_insomnia("[1, 2", dir.path()).unwrap_err().is_input());
    assert!(import_insomnia("42", dir.path()).unwrap_err().is_input());
}

#[test]
fn ef_imp_02_two_imports_never_overwrite_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = import_insomnia(V4, dir.path()).unwrap();
    let (second, _) = import_insomnia(V4, dir.path()).unwrap();
    assert_ne!(first, second);
}
