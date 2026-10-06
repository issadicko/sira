//! Les secrets des environnements : leurs valeurs vivent dans le trousseau du système, jamais dans les fichiers ni dans
//! l'interface (elle sait seulement qu'une valeur existe).

use std::path::Path;
use std::sync::Arc;

use tauri::State;
use xc_core::EnvVar;
use xc_secrets::{Keychain, SecretKey, SecretStore};

use crate::{err, AppState, Reply};

/// Le trousseau de l'application ; les tests y mettent un trousseau en mémoire.
pub struct Secrets(pub Arc<dyn SecretStore>);

impl Default for Secrets {
    fn default() -> Self {
        Self(Arc::new(Keychain))
    }
}

fn key(root: &str, env: &str, name: &str) -> SecretKey {
    SecretKey::new(&xc_secrets::collection_id(root), env, name)
}

/// Noms des variables que l'environnement déclare secrètes ; un environnement illisible n'en a aucune.
pub fn declared(root: &str, env: &str) -> Vec<String> {
    xc_core::read_environment(Path::new(root), env)
        .map(|vars| vars.into_iter().filter(|v| v.secret).map(|v| v.name).collect())
        .unwrap_or_default()
}

/// Les valeurs gardées pour les secrets de `env`, avec leur nom ; la première erreur du trousseau l'arrête.
pub fn take(store: &dyn SecretStore, root: &str, env: &str) -> Reply<Vec<(String, String)>> {
    let mut values = Vec::new();
    for name in declared(root, env) {
        if let Some(value) = store.get(&key(root, env, &name)).map_err(err)? {
            values.push((name, value));
        }
    }
    Ok(values)
}

/// Les valeurs pour l'exécution des requêtes. Un trousseau indisponible ne bloque pas l'envoi : les secrets restent alors
/// sans valeur et la requête les signale comme variables non résolues.
pub fn load(store: &dyn SecretStore, root: &str, env: Option<&str>) -> Vec<(String, String)> {
    env.and_then(|env| take(store, root, env).ok()).unwrap_or_default()
}

/// Range `values` sous `env`.
pub fn put(store: &dyn SecretStore, root: &str, env: &str, values: &[(String, String)]) -> Reply<()> {
    for (name, value) in values {
        store.set(&key(root, env, name), value).map_err(err)?;
    }
    Ok(())
}

/// Range les valeurs de secrets de `vars` dans le trousseau et rend les variables telles qu'elles s'écrivent dans le
/// fichier (sans valeur pour un secret). `Some(texte)` enregistre, `Some("")` efface, `None` laisse la valeur gardée.
/// Les secrets qui n'en sont plus (retirés, ou devenus ordinaires) sont oubliés, au mieux. Rend aussi si le trousseau a changé.
pub fn apply(store: &dyn SecretStore, root: &str, env: &str, vars: &[EnvVar]) -> Reply<(Vec<EnvVar>, bool)> {
    let wanted = |v: &EnvVar| v.secret && v.value.as_deref().is_some_and(|s| !s.is_empty());
    let cleared = |v: &EnvVar| v.secret && v.value.as_deref() == Some("");
    let mut changed = false;
    for var in vars {
        if wanted(var) {
            store.set(&key(root, env, &var.name), var.value.as_deref().unwrap_or_default()).map_err(err)?;
            changed = true;
        } else if cleared(var) {
            store.delete(&key(root, env, &var.name)).map_err(err)?;
            changed = true;
        }
    }
    for name in declared(root, env) {
        if !vars.iter().any(|v| v.secret && v.name == name) {
            store.delete(&key(root, env, &name)).ok();
            changed = true;
        }
    }
    let stripped =
        vars.iter().map(|v| if v.secret { EnvVar { value: None, ..v.clone() } } else { v.clone() }).collect();
    Ok((stripped, changed))
}

/// Oublie les secrets `names` de `env` (au mieux : une valeur qui reste est inoffensive, personne ne la lit).
pub fn forget(store: &dyn SecretStore, root: &str, env: &str, names: &[String]) {
    for name in names {
        store.delete(&key(root, env, name)).ok();
    }
}

/// Les secrets de `env` dont le trousseau garde une valeur : l'interface ne reçoit jamais plus que leurs noms.
#[tauri::command]
pub async fn secret_names(state: State<'_, AppState>, root: String, env: String) -> Reply<Vec<String>> {
    let store = Arc::clone(&state.secrets.0);
    crate::blocking(move || {
        let mut stored = Vec::new();
        for name in declared(&root, &env) {
            if store.get(&key(&root, &env, &name)).map_err(err)?.is_some() {
                stored.push(name);
            }
        }
        Ok::<_, String>(stored)
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Write};

    use tauri::Manager;
    use xc_secrets::MemoryStore;

    use super::*;

    const ENV: &str = "name: dev\nvariables:\n  - name: base\n    value: http://x\n  - secret: true\n    name: token\n";

    fn var(name: &str, value: Option<&str>, secret: bool) -> EnvVar {
        EnvVar {
            name: name.into(),
            value: value.map(str::to_owned),
            secret,
            enabled: true,
            description: None,
            data_type: None,
        }
    }

    fn collection(env: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: S\n").unwrap();
        fs::create_dir(dir.path().join("environments")).unwrap();
        fs::write(dir.path().join("environments/dev.yml"), env).unwrap();
        let root = dir.path().display().to_string();
        (dir, root)
    }

    fn app(store: &Arc<MemoryStore>) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        app.manage(AppState { secrets: Secrets(store.clone()), ..AppState::default() });
        app
    }

    fn get(store: &MemoryStore, root: &str, env: &str, name: &str) -> Option<String> {
        store.get(&key(root, env, name)).unwrap()
    }

    #[test]
    fn enf_sec_01_apply_stores_typed_values_clears_and_strips_them_from_the_file_form() {
        let (_dir, root) = collection(ENV);
        let store = MemoryStore::default();
        let vars = [var("base", Some("http://x"), false), var("token", Some("s3cr3t"), true)];
        let (written, changed) = apply(&store, &root, "dev", &vars).unwrap();
        assert!(changed);
        assert_eq!(get(&store, &root, "dev", "token").as_deref(), Some("s3cr3t"));
        assert_eq!(written[1].value, None, "la valeur d'un secret ne va pas dans le fichier");
        assert_eq!(written[0].value.as_deref(), Some("http://x"));

        let (_, changed) =
            apply(&store, &root, "dev", &[var("base", Some("http://x"), false), var("token", None, true)]).unwrap();
        assert!(!changed, "sans valeur saisie, la valeur gardée est conservée");
        assert_eq!(get(&store, &root, "dev", "token").as_deref(), Some("s3cr3t"));

        apply(&store, &root, "dev", &[var("token", Some(""), true)]).unwrap();
        assert_eq!(get(&store, &root, "dev", "token"), None, "une valeur vide efface");
    }

    #[test]
    fn enf_sec_01_a_secret_that_is_removed_or_made_ordinary_is_forgotten() {
        let (_dir, root) = collection(ENV);
        let store = MemoryStore::default();
        store.set(&key(&root, "dev", "token"), "s3cr3t").unwrap();
        let (_, changed) = apply(&store, &root, "dev", &[var("token", Some("plain"), false)]).unwrap();
        assert!(changed);
        assert_eq!(get(&store, &root, "dev", "token"), None);
    }

    #[test]
    fn enf_sec_01_only_the_secrets_a_file_declares_are_loaded() {
        let (_dir, root) = collection(ENV);
        let store = MemoryStore::default();
        store.set(&key(&root, "dev", "token"), "s3cr3t").unwrap();
        store.set(&key(&root, "dev", "base"), "ignored").unwrap();
        store.set(&key(&root, "prod", "token"), "other").unwrap();
        assert_eq!(load(&store, &root, Some("dev")), [("token".to_owned(), "s3cr3t".to_owned())]);
        assert!(load(&store, &root, None).is_empty());
    }

    #[test]
    fn enf_sec_01_a_broken_keychain_never_blocks_loading_and_only_fails_typed_values() {
        let (_dir, root) = collection(ENV);
        let store = MemoryStore::broken();
        assert!(load(&store, &root, Some("dev")).is_empty());
        assert!(
            apply(&store, &root, "dev", &[var("token", None, true)]).is_ok(),
            "rien à ranger : le trousseau n'est pas touché"
        );
        let failure = apply(&store, &root, "dev", &[var("token", Some("x"), true)]).unwrap_err();
        assert!(failure.contains("trousseau indisponible"), "{failure}");
    }

    #[tokio::test]
    async fn enf_sec_01_saving_keeps_the_value_out_of_the_file_and_reports_what_is_stored() {
        let store = Arc::new(MemoryStore::default());
        let app = app(&store);
        let state = || app.state::<AppState>();
        let (dir, root) = collection(ENV);
        let vars = vec![var("base", Some("http://x"), false), var("token", Some("s3cr3t"), true)];

        assert!(
            crate::save_environment(state(), root.clone(), "dev".into(), vars, false).await.unwrap(),
            "le trousseau a changé"
        );
        assert!(!fs::read_to_string(dir.path().join("environments/dev.yml")).unwrap().contains("s3cr3t"));
        assert_eq!(secret_names(state(), root.clone(), "dev".into()).await.unwrap(), ["token"]);

        let unchanged = vec![var("base", Some("http://x"), false), var("token", None, true)];
        assert!(!crate::save_environment(state(), root, "dev".into(), unchanged, false).await.unwrap());
    }

    #[tokio::test]
    async fn enf_sec_01_a_broken_keychain_refuses_a_typed_secret_before_touching_the_file() {
        let store = Arc::new(MemoryStore::broken());
        let app = app(&store);
        let state = || app.state::<AppState>();
        let (dir, root) = collection(ENV);
        let before = fs::read_to_string(dir.path().join("environments/dev.yml")).unwrap();
        let vars = vec![var("base", Some("http://changed"), false), var("token", Some("s3cr3t"), true)];

        let failure = crate::save_environment(state(), root.clone(), "dev".into(), vars, false).await.unwrap_err();
        assert!(failure.contains("trousseau indisponible"), "{failure}");
        assert_eq!(
            fs::read_to_string(dir.path().join("environments/dev.yml")).unwrap(),
            before,
            "rien n'est enregistré à moitié"
        );

        let plain = vec![var("base", Some("http://changed"), false), var("token", None, true)];
        assert!(
            crate::save_environment(state(), root, "dev".into(), plain, false).await.unwrap(),
            "sans valeur à ranger, l'enregistrement passe"
        );
    }

    #[tokio::test]
    async fn enf_sec_01_renaming_cloning_and_deleting_an_environment_carry_its_secrets() {
        let store = Arc::new(MemoryStore::default());
        let app = app(&store);
        let state = || app.state::<AppState>();
        let (_dir, root) = collection(ENV);
        store.set(&key(&root, "dev", "token"), "s3cr3t").unwrap();

        let copy = crate::clone_environment(state(), root.clone(), "dev".into(), "copie".into()).await.unwrap();
        assert_eq!(get(&store, &root, &copy, "token").as_deref(), Some("s3cr3t"));
        assert_eq!(get(&store, &root, "dev", "token").as_deref(), Some("s3cr3t"), "la duplication ne déplace rien");

        let renamed = crate::rename_environment(state(), root.clone(), "dev".into(), "staging".into()).await.unwrap();
        assert_eq!(get(&store, &root, &renamed, "token").as_deref(), Some("s3cr3t"));
        assert_eq!(get(&store, &root, "dev", "token"), None, "le renommage déplace");
    }

    #[tokio::test]
    async fn enf_sec_01_a_request_resolves_a_secret_from_the_keychain() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                let token = text
                    .lines()
                    .find_map(|l| l.strip_prefix("X-Token: ").or_else(|| l.strip_prefix("x-token: ")))
                    .unwrap_or("absent");
                let body = format!("{{\"token\":\"{token}\"}}");
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        let store = Arc::new(MemoryStore::default());
        let app = app(&store);
        let state = || app.state::<AppState>();
        let (dir, root) = collection(&ENV.replace("http://x", &base));
        fs::write(
            dir.path().join("api.yml"),
            "info:\n  name: Api\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/a\"\n  headers:\n    - name: X-Token\n      value: \"{{token}}\"\n",
        )
        .unwrap();
        let doc = || xc_core::read_request(dir.path(), "api.yml").unwrap();
        let send = |id: &str| crate::SendArgs {
            id: id.into(),
            root: root.clone(),
            path: "api.yml".into(),
            doc: doc(),
            env: Some("dev".into()),
        };

        let without = crate::send_request(app.handle().clone(), state(), send("a")).await.unwrap();
        assert_eq!(without.unresolved, ["token"], "sans valeur gardée, le secret est signalé");

        store.set(&key(&root, "dev", "token"), "s3cr3t").unwrap();
        let with = crate::send_request(app.handle().clone(), state(), send("b")).await.unwrap();
        assert!(with.unresolved.is_empty(), "{:?}", with.unresolved);
        assert_eq!(with.response.unwrap().body, r#"{"token":"s3cr3t"}"#);

        let infos = crate::variables(state(), root.clone(), "api.yml".into(), doc(), Some("dev".into())).await.unwrap();
        let token = infos.iter().find(|v| v.name == "token").unwrap();
        assert_eq!(token.value.as_deref(), Some(xc_core::vars::SECRET_MASK), "l'interface ne reçoit pas la valeur");
        assert!(!serde_json::to_string(&infos).unwrap().contains("s3cr3t"));
    }
}
