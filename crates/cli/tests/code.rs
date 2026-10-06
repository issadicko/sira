use std::fs;
use std::process::Command;

fn xc(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xc")).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn collection() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: Boutique\n").unwrap();
    fs::create_dir(root.join("environments")).unwrap();
    fs::write(
        root.join("environments/prod.yml"),
        "name: prod\nvariables:\n  - name: base\n    value: https://api.boutique.test\n",
    )
    .unwrap();
    fs::write(
        root.join("creer.yml"),
        "info:\n  name: Créer\n  type: http\n\nhttp:\n  method: POST\n  url: \"{{base}}/produits?canal={{canal}}\"\n  headers:\n    - name: X-Marchand\n      value: \"{{marchand}}\"\n  body:\n    type: json\n    data: '{\"nom\": \"café\"}'\n",
    )
    .unwrap();
    dir
}

#[test]
fn ef_gen_01_cli_code_prints_the_request_in_the_chosen_language_with_variables_resolved() {
    let dir = collection();
    let root = dir.path().to_str().unwrap();
    let (code, stdout, stderr) =
        xc(&["code", root, "creer.yml", "--env", "prod", "--env-var", "canal=web", "--env-var", "marchand=m-1"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    assert!(
        stdout.starts_with("curl --request POST \\\n  --url 'https://api.boutique.test/produits?canal=web'"),
        "{stdout}"
    );
    assert!(
        stdout.contains("--header 'X-Marchand: m-1'") && stdout.contains("--data-raw '{\"nom\": \"café\"}'"),
        "{stdout}"
    );

    for (language, marker) in [
        ("javascript", "fetch("),
        ("python", "requests.request(\"POST\""),
        ("go", "http.NewRequest(\"POST\""),
        ("java", "new OkHttpClient()"),
        ("kotlin", "OkHttpClient()"),
        ("dart", "http.Request('POST'"),
        ("php", "curl_init()"),
        ("csharp", "new HttpMethod(\"POST\")"),
    ] {
        let (code, stdout, stderr) = xc(&[
            "code",
            root,
            "creer.yml",
            "--lang",
            language,
            "--env",
            "prod",
            "--env-var",
            "canal=web",
            "--env-var",
            "marchand=m-1",
        ]);
        assert_eq!(code, 0, "{language} : {stderr}");
        assert!(stdout.contains(marker), "{language} : {stdout}");
    }
}

#[test]
fn ef_gen_01_cli_code_warns_about_variables_without_a_value_and_keeps_them_braced() {
    let dir = collection();
    let (code, stdout, stderr) = xc(&["code", dir.path().to_str().unwrap(), "creer.yml", "--env", "prod"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("canal={{canal}}") && stdout.contains("X-Marchand: {{marchand}}"), "{stdout}");
    assert!(
        stderr.contains("la variable canal n'a pas de valeur")
            && stderr.contains("la variable marchand n'a pas de valeur"),
        "{stderr}"
    );
}

#[test]
fn ef_gen_01_cli_code_refuses_an_unknown_language_or_request_with_code_2() {
    let dir = collection();
    let root = dir.path().to_str().unwrap();
    let (code, _, stderr) = xc(&["code", root, "creer.yml", "--lang", "cobol"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("langage inconnu : cobol") && stderr.contains("csharp"), "{stderr}");
    let (code, _, stderr) = xc(&["code", root, "absente.yml"]);
    assert_eq!(code, 2, "{stderr}");
}
