//! Le gestionnaire de cookies : voir, ajouter, modifier et supprimer ce que contient le pot de l'application. Le pot
//! n'existe qu'en mémoire : il est vide à chaque lancement.

use serde::Deserialize;
use tauri::State;
use xc_engine::{CookieDraft, CookieView};

use crate::{AppState, Reply};

/// L'identité d'un cookie dans le pot : son domaine, son chemin et son nom.
#[derive(Debug, Deserialize)]
pub struct CookieKey {
    domain: String,
    path: String,
    key: String,
}

#[tauri::command]
pub fn cookies_list(state: State<'_, AppState>) -> Vec<CookieView> {
    state.cookies.list()
}

/// Ajoute un cookie, ou remplace `previous` quand il est donné ; rend le pot à jour.
#[tauri::command]
pub fn cookie_save(
    state: State<'_, AppState>,
    previous: Option<CookieKey>,
    cookie: CookieDraft,
) -> Reply<Vec<CookieView>> {
    match previous {
        Some(old) => state.cookies.replace((&old.domain, &old.path, &old.key), &cookie)?,
        None => state.cookies.put(&cookie)?,
    }
    Ok(state.cookies.list())
}

#[tauri::command]
pub fn cookie_delete(state: State<'_, AppState>, cookie: CookieKey) -> Vec<CookieView> {
    state.cookies.delete(&cookie.domain, &cookie.path, &cookie.key);
    state.cookies.list()
}

#[tauri::command]
pub fn cookies_delete_domain(state: State<'_, AppState>, domain: String) -> Vec<CookieView> {
    state.cookies.delete_domain(&domain);
    state.cookies.list()
}

#[tauri::command]
pub fn cookies_clear(state: State<'_, AppState>) -> Vec<CookieView> {
    state.cookies.clear();
    state.cookies.list()
}

#[cfg(test)]
mod tests {
    use tauri::Manager;

    use super::*;

    fn draft(key: &str, domain: &str) -> CookieDraft {
        CookieDraft {
            key: key.into(),
            value: "v".into(),
            domain: domain.into(),
            path: "/".into(),
            ..CookieDraft::default()
        }
    }

    fn keys(cookies: &[CookieView]) -> Vec<String> {
        cookies.iter().map(|c| format!("{}@{}", c.key, c.domain)).collect()
    }

    #[test]
    fn ef_ux_02_the_manager_adds_edits_and_deletes_cookies_of_the_shared_jar() {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let state = || app.state::<AppState>();

        let added = cookie_save(state(), None, draft("a", "a.test")).unwrap();
        assert_eq!(keys(&added), vec!["a@a.test"]);
        cookie_save(state(), None, draft("b", "b.test")).unwrap();

        let key = || CookieKey { domain: "a.test".into(), path: "/".into(), key: "a".into() };
        let edited =
            cookie_save(state(), Some(key()), CookieDraft { value: "changed".into(), ..draft("a", "a.test") }).unwrap();
        assert_eq!(edited.iter().find(|c| c.key == "a").map(|c| c.value.as_str()), Some("changed"));

        assert_eq!(keys(&cookie_delete(state(), key())), vec!["b@b.test"]);
        assert!(cookies_delete_domain(state(), "b.test".into()).is_empty());
        cookie_save(state(), None, draft("c", "c.test")).unwrap();
        assert!(cookies_clear(state()).is_empty());
        assert!(cookies_list(state()).is_empty());
    }

    #[tokio::test]
    async fn ef_ux_02_a_cookie_set_by_a_send_shows_in_the_manager_and_comes_back_on_the_next_send() {
        use std::io::{Read, Write};
        use std::sync::{Arc, Mutex};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for (i, stream) in listener.incoming().take(2).enumerate() {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap();
                log.lock().unwrap().push(String::from_utf8_lossy(&buf[..n]).to_lowercase());
                let set = if i == 0 { "set-cookie: sid=abc; Path=/\r\n" } else { "" };
                write!(stream, "HTTP/1.1 200 OK\r\n{set}content-length: 2\r\nconnection: close\r\n\r\nok").unwrap();
            }
        });
        let (data, collection) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let app = tauri::test::mock_app();
        app.manage(AppState { data: std::sync::Mutex::new(Some(data.path().to_path_buf())), ..AppState::default() });
        let state = || app.state::<AppState>();
        let root = collection.path();
        std::fs::write(root.join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: S\n").unwrap();
        std::fs::write(
            root.join("req.yml"),
            format!("info:\n  name: R\n  type: http\n\nhttp:\n  method: GET\n  url: {base}/x\n"),
        )
        .unwrap();
        let doc = xc_core::read_request(root, "req.yml").unwrap();
        let send = |id: &str| crate::SendArgs {
            id: id.into(),
            root: root.display().to_string(),
            path: "req.yml".into(),
            doc: doc.clone(),
            env: None,
        };

        crate::send_request(app.handle().clone(), state(), send("1")).await.unwrap();
        assert_eq!(keys(&cookies_list(state())), vec!["sid@127.0.0.1"]);
        crate::send_request(app.handle().clone(), state(), send("2")).await.unwrap();

        let seen = seen.lock().unwrap();
        assert!(!seen[0].contains("cookie:") && seen[1].contains("cookie: sid=abc"), "{seen:?}");
    }

    #[test]
    fn ef_ux_02_a_cookie_the_jar_refuses_is_reported_to_the_manager() {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let error = cookie_save(app.state::<AppState>(), None, draft("", "a.test")).unwrap_err();
        assert!(error.contains("nom"), "{error}");
    }
}
