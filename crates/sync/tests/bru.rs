use std::fs;
use std::path::{Path, PathBuf};

use xc_core::collection::TreeItem;
use xc_core::{open_collection, read_environment, read_request, Auth, Body, MultipartValue, ParamKind};
use xc_sync::bru::is_bru_collection;
use xc_sync::import::import_bru;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bru-shop")
}

/// Copie du dossier de test, pour y ajouter des fichiers sans toucher à la source versionnée.
fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn imported() -> (tempfile::TempDir, PathBuf, Vec<xc_sync::postman::Issue>) {
    let dir = tempfile::tempdir().unwrap();
    let (root, issues) = import_bru(&fixture(), dir.path()).unwrap();
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
fn ef_imp_03_a_bru_collection_becomes_an_opencollection_folder_tree() {
    let (_dir, root, _) = imported();
    let info = open_collection(&root).unwrap();
    assert_eq!(info.name, "Boutique Bru");
    assert_eq!(root.file_name().unwrap(), "Boutique Bru");
    let mut all = Vec::new();
    paths(&info.items, &mut all);
    for expected in [
        "Utilisateurs",
        "Utilisateurs/List users.yml",
        "Utilisateurs/Create user.yml",
        "Utilisateurs/Upload.yml",
        "Login.yml",
        "Orders.yml",
        "Future.yml",
    ] {
        assert!(all.iter().any(|p| p == expected), "{expected} absent de {all:?}");
    }
    assert!(!all.iter().any(|p| p.contains("Broken") || p.contains("Stream")), "{all:?}");
    assert_eq!(info.request_count, 6);
    assert!(root.join("opencollection.yml").is_file() && !root.join("bruno.json").exists());
}

#[test]
fn ef_imp_03_issues_name_what_could_not_be_converted_and_the_source_is_untouched() {
    let before: Vec<_> = {
        let mut files = Vec::new();
        collect(&fixture(), &mut files);
        files
    };
    let (_dir, _, issues) = imported();
    let list: Vec<_> = issues.iter().map(|i| (i.path.as_str(), i.severity)).collect();
    assert_eq!(
        list,
        [("Broken.bru", "error"), ("Future.bru", "warning"), ("Stream.bru", "error"), ("(collection)", "warning")],
        "{issues:?}"
    );
    assert!(issues[0].message.contains("ligne 7"), "{}", issues[0].message);
    assert!(issues[1].message.contains("shiny-new-feature"), "{}", issues[1].message);
    assert!(issues[2].message.contains("grpc"), "{}", issues[2].message);
    assert!(issues[3].message.contains(".csv ×1") && !issues[3].message.contains(".json"), "{}", issues[3].message);

    let mut after = Vec::new();
    collect(&fixture(), &mut after);
    assert_eq!(before, after, "la source n'est pas modifiée");
}

fn collect(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, out);
        } else {
            out.push((path.clone(), fs::read_to_string(&path).unwrap_or_default()));
        }
    }
}

#[test]
fn ef_imp_03_a_request_keeps_its_url_params_headers_scripts_assertions_and_settings() {
    let (_dir, root, _) = imported();
    let list = read(&root, "Utilisateurs/List users.yml");
    assert_eq!((list.method.as_str(), list.url.as_str()), ("GET", "{{baseUrl}}/users/:team?page=1"));
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
    assert_eq!(list.docs.as_deref(), Some("Liste les utilisateurs"));
    assert_eq!(list.seq, Some(1));
    let tests = list.scripts.iter().find(|s| s.kind == "tests").expect("script de tests");
    assert!(tests.code.contains("test(\"returns users\""), "{}", tests.code);
    assert_eq!(list.assertions.len(), 2);
    assert_eq!((list.assertions[0].expression.as_str(), list.assertions[0].operator.as_str()), ("res.status", "eq"));

    let text = fs::read_to_string(root.join("Utilisateurs/List users.yml")).unwrap();
    assert!(text.contains("timeout: 3000") && text.contains("- smoke") && text.contains("- users"), "{text}");
}

#[test]
fn ef_imp_03_bodies_of_every_mode_are_converted() {
    let (_dir, root, _) = imported();
    let create = read(&root, "Utilisateurs/Create user.yml");
    let Body::Json { data } = &create.body else { panic!("{:?}", create.body) };
    assert!(data.starts_with("{\n  \"name\": \"Aminata\"") && data.ends_with('}'), "{data}");
    assert!(fs::read_to_string(root.join("Utilisateurs/Create user.yml")).unwrap().contains("encodeUrl: false"));

    let Body::MultipartForm { fields } = read(&root, "Utilisateurs/Upload.yml").body else {
        panic!("multipart attendu")
    };
    assert_eq!(fields[0].value, MultipartValue::Text("Photo".into()));
    assert_eq!(fields[1].value, MultipartValue::File(vec!["avatars/a.png".into(), "avatars/b.png".into()]));
    assert_eq!(fields[1].content_type.as_deref(), Some("image/png"));

    let Body::FormUrlEncoded { fields } = read(&root, "Login.yml").body else { panic!("formulaire attendu") };
    assert_eq!(
        fields.iter().map(|f| (f.name.as_str(), f.enabled)).collect::<Vec<_>>(),
        [("grant", true), ("scope", false)]
    );

    let graphql = fs::read_to_string(root.join("Orders.yml")).unwrap();
    assert!(
        graphql.contains("type: graphql")
            && graphql.contains("orders(first: $first)")
            && graphql.contains("\"first\": 3"),
        "{graphql}"
    );
}

#[test]
fn ef_imp_03_auth_is_carried_at_every_level() {
    let (_dir, root, _) = imported();
    let collection = fs::read_to_string(root.join("opencollection.yml")).unwrap();
    assert!(collection.contains("type: bearer") && collection.contains("{{token}}"), "{collection}");
    assert!(collection.contains("baseUrl") && collection.contains("bru.setVar(\"started\""), "{collection}");
    assert!(collection.contains("API de démonstration") && collection.contains("flow: sequential"), "{collection}");
    assert!(collection.contains("url: https://shop.test"), "les préréglages de bruno.json sont repris : {collection}");

    let folder = fs::read_to_string(root.join("Utilisateurs/folder.yml")).unwrap();
    assert!(
        folder.contains("name: Utilisateurs") && folder.contains("seq: 1") && folder.contains("X-Area"),
        "{folder}"
    );

    assert!(matches!(read(&root, "Utilisateurs/List users.yml").auth, Auth::Inherit));
    assert!(
        matches!(&read(&root, "Utilisateurs/Create user.yml").auth, Auth::Basic { username, password } if username == "admin" && password == "{{password}}")
    );
    assert!(matches!(read(&root, "Utilisateurs/Upload.yml").auth, Auth::None));

    let Auth::Oauth2(oauth) = read(&root, "Login.yml").auth else { panic!("OAuth 2 attendu") };
    assert_eq!(oauth.flow, "client_credentials");
    assert_eq!((oauth.client_id.as_str(), oauth.credentials_placement.as_str()), ("shop", "basic_auth_header"));
    assert_eq!(oauth.parameters.len(), 1);
    assert_eq!(
        (oauth.parameters[0].name.as_str(), oauth.parameters[0].stage.as_str(), oauth.parameters[0].placement.as_str()),
        ("audience", "token", "body")
    );
}

#[test]
fn ef_imp_03_post_response_variables_become_actions_and_pre_request_ones_variables() {
    let (_dir, root, _) = imported();
    let text = fs::read_to_string(root.join("Utilisateurs/Create user.yml")).unwrap();
    assert!(text.contains("name: retries") && text.contains("expression: res.body.id"), "{text}");
    assert!(text.contains("scope: runtime") && text.contains("scope: request"), "{text}");
    assert!(text.contains("bru.setEnvVar(\"lastUser\""), "{text}");
}

#[test]
fn ef_imp_03_environments_keep_their_variables_and_secrets_without_values() {
    let (_dir, root, _) = imported();
    let local = read_environment(&root, "Local").unwrap();
    let list: Vec<_> = local.iter().map(|v| (v.name.as_str(), v.value.as_deref(), v.secret, v.enabled)).collect();
    assert_eq!(
        list,
        [
            ("baseUrl", Some("http://localhost:8080"), false, true),
            ("debug", Some("true"), false, false),
            ("token", None, true, true)
        ]
    );
    let prod = read_environment(&root, "Prod").unwrap();
    assert_eq!(prod.iter().filter(|v| v.secret).map(|v| v.name.as_str()).collect::<Vec<_>>(), ["token", "apiKey"]);
}

#[test]
fn ef_imp_03_each_request_keeps_its_seq_and_the_files_read_back_without_loss() {
    let (_dir, root, _) = imported();
    let seqs: Vec<_> = ["Login.yml", "Orders.yml", "Future.yml", "Utilisateurs/Upload.yml"]
        .iter()
        .map(|p| read(&root, p).seq)
        .collect();
    assert_eq!(seqs, [Some(1), Some(2), Some(4), Some(3)]);
    assert_eq!(open_collection(&root).unwrap().items.len(), 4);
    for path in ["Utilisateurs/List users.yml", "Utilisateurs/Create user.yml", "Login.yml", "Orders.yml"] {
        let doc = read(&root, path);
        let text = fs::read_to_string(root.join(path)).unwrap();
        xc_core::save_request(&root, path, &doc).unwrap();
        assert_eq!(fs::read_to_string(root.join(path)).unwrap(), text, "{path} ne revient pas à l'identique");
    }
}

#[test]
fn ef_imp_03_ignored_folders_hidden_folders_and_crlf_files_are_handled() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("src");
    copy(&fixture(), &source);
    for hidden in ["node_modules/pkg", ".git/hooks", ".cache"] {
        fs::create_dir_all(source.join(hidden)).unwrap();
        fs::write(
            source.join(hidden).join("Ghost.bru"),
            "meta {\n  name: Ghost\n  type: http\n}\n\nget {\n  url: x\n}\n",
        )
        .unwrap();
    }
    fs::create_dir_all(source.join("Sub/environments")).unwrap();
    fs::write(
        source.join("Sub/environments/Deep.bru"),
        "meta {\r\n  name: Deep\r\n  type: http\r\n  seq: 1\r\n}\r\n\r\nget {\r\n  url: https://x.test/deep\r\n  body: none\r\n  auth: none\r\n}\r\n\r\ndocs {\r\n  ligne un\r\n  ligne deux\r\n}\r\n",
    )
    .unwrap();

    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    let (root, _) = import_bru(&source, &out).unwrap();
    let mut all = Vec::new();
    paths(&open_collection(&root).unwrap().items, &mut all);
    assert!(!all.iter().any(|p| p.contains("Ghost")), "{all:?}");
    let deep = read(&root, "Sub/environments/Deep.yml");
    assert_eq!(deep.url, "https://x.test/deep");
    assert_eq!(deep.docs.as_deref(), Some("ligne un\nligne deux"));
}

#[test]
fn ef_imp_03_two_imports_never_overwrite_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = import_bru(&fixture(), dir.path()).unwrap();
    let (second, _) = import_bru(&fixture(), dir.path()).unwrap();
    assert_ne!(first, second);
    assert!(first.is_dir() && second.is_dir());
}

#[test]
fn ef_imp_03_a_folder_that_is_not_a_bru_collection_is_refused_as_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let error = import_bru(dir.path(), dir.path()).unwrap_err();
    assert!(error.is_input() && error.to_string().contains("bruno.json"), "{error}");

    fs::write(dir.path().join("bruno.json"), "pas du json").unwrap();
    let error = import_bru(dir.path(), dir.path()).unwrap_err();
    assert!(error.is_input() && error.to_string().contains("bruno.json illisible"), "{error}");

    assert!(is_bru_collection(&fixture()));
    fs::write(dir.path().join("opencollection.yml"), "opencollection: 1.0.0\n").unwrap();
    assert!(!is_bru_collection(dir.path()), "un dossier déjà en YAML n'est pas à convertir");
}

#[test]
fn ef_imp_03_saved_examples_become_yaml_examples() {
    let (_dir, root, _) = imported();
    let text = fs::read_to_string(root.join("Utilisateurs/List users.yml")).unwrap();
    assert!(text.contains("examples:") && text.contains("name: Two users"), "{text}");
    assert!(text.contains("status: 200") && text.contains("statusText: OK"), "{text}");
    assert!(text.contains("[{\"id\": 1}, {\"id\": 2}]"), "{text}");
    let doc = read(&root, "Utilisateurs/List users.yml");
    xc_core::save_request(&root, "Utilisateurs/List users.yml", &doc).unwrap();
    assert_eq!(fs::read_to_string(root.join("Utilisateurs/List users.yml")).unwrap(), text);
}
