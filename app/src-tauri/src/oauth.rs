//! OAuth 2 dans l'application : la fenêtre de connexion des flux interactifs (code d'autorisation, implicite) et les
//! commandes qui montrent, obtiennent et effacent le jeton d'une requête.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use tauri::{AppHandle, Runtime, State, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tokio::sync::oneshot;
use xc_core::oauth2::OAuth2;
use xc_core::{prepare_with, RequestDoc, SendAuth};
use xc_engine::Network;
use xc_runner::{
    redirect_params, token_key, Authorization, AuthorizationRequest, Authorizer, Session, SharedAuthorizer, TokenInfo,
};

use crate::{err, AppState, Reply};

type Landing = Result<HashMap<String, String>, String>;

static WINDOWS: AtomicU64 = AtomicU64::new(0);

/// Ouvre l'adresse d'autorisation dans une fenêtre et attend que le serveur renvoie l'utilisateur vers l'adresse de
/// rappel : la navigation y est interceptée, rien n'a besoin d'écouter à cette adresse.
pub struct WindowAuthorizer<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> std::fmt::Debug for WindowAuthorizer<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowAuthorizer").finish_non_exhaustive()
    }
}

/// L'autoriseur à poser sur une session de l'application.
pub fn authorizer<R: Runtime>(app: &AppHandle<R>) -> SharedAuthorizer {
    Arc::new(WindowAuthorizer { app: app.clone() })
}

fn settle(slot: &Mutex<Option<oneshot::Sender<Landing>>>, outcome: Landing) {
    if let Some(tx) = slot.lock().ok().and_then(|mut slot| slot.take()) {
        tx.send(outcome).ok();
    }
}

async fn sign_in<R: Runtime>(app: AppHandle<R>, request: AuthorizationRequest) -> Landing {
    let url: Url = request.url.parse().map_err(|e| format!("adresse d'autorisation invalide ({e})"))?;
    let (tx, rx) = oneshot::channel();
    let slot = Arc::new(Mutex::new(Some(tx)));

    let (landing, callback) = (Arc::clone(&slot), request.callback_url);
    let window = WebviewWindowBuilder::new(
        &app,
        format!("oauth-{}", WINDOWS.fetch_add(1, Ordering::Relaxed)),
        WebviewUrl::External(url),
    )
    .title("Connexion")
    .inner_size(520.0, 740.0)
    .center()
    .on_navigation(move |landed| match redirect_params(landed.as_str(), &callback) {
        Some(params) => {
            settle(&landing, Ok(params));
            false
        }
        None => true,
    })
    .build()
    .map_err(err)?;
    let closing = Arc::clone(&slot);
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed) {
            settle(&closing, Err("la fenêtre de connexion a été fermée avant la fin de l'autorisation".into()));
        }
    });

    let outcome = rx.await.unwrap_or_else(|_| Err("la fenêtre de connexion s'est arrêtée".into()));
    window.destroy().ok();
    outcome
}

impl<R: Runtime> Authorizer for WindowAuthorizer<R> {
    fn authorize(&self, request: AuthorizationRequest) -> Authorization<'_> {
        Box::pin(sign_in(self.app.clone(), request))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenArgs {
    root: String,
    path: String,
    doc: RequestDoc,
    env: Option<String>,
}

/// La configuration OAuth 2 de la requête telle qu'elle serait envoyée (variables résolues, auth héritée comprise),
/// avec la session de la collection.
fn resolve(state: &AppState, args: &TokenArgs) -> Reply<(Box<OAuth2>, Session, Network)> {
    let session = state.sessions.lock().map_err(err)?.get(&args.root).cloned().unwrap_or_default();
    let overrides = session.request_overrides(None, args.env.as_deref());
    let prepared = prepare_with(
        Path::new(&args.root),
        &args.path,
        &args.doc,
        args.env.as_deref(),
        &session.runtime_strings(),
        overrides,
    )
    .map_err(err)?;
    match prepared.auth {
        SendAuth::Oauth2(config) => Ok((config, session, prepared.request.network)),
        _ => Err("cette requête n'utilise pas OAuth 2".into()),
    }
}

/// Le jeton gardé pour la requête, sans sa valeur ; `None` quand il n'y en a pas, ou que la requête n'est pas en OAuth 2.
#[tauri::command]
pub fn oauth_status(state: State<'_, AppState>, args: TokenArgs) -> Reply<Option<TokenInfo>> {
    Ok(resolve(&state, &args)
        .ok()
        .and_then(|(config, session, _)| session.tokens.get(&token_key(&config)).map(xc_runner::Token::info)))
}

/// Demande un jeton neuf au serveur d'autorisation (ouvre la fenêtre de connexion pour les flux interactifs) et le garde.
#[tauri::command]
pub async fn oauth_fetch<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    args: TokenArgs,
) -> Reply<TokenInfo> {
    let (config, _, network) = resolve(&state, &args)?;
    let token = xc_runner::fetch_oauth2_token(&config, Some(&authorizer(&app)), &network).await?;
    let info = token.info();
    state.sessions.lock().map_err(err)?.entry(args.root).or_default().tokens.insert(token_key(&config), token);
    Ok(info)
}

/// Oublie le jeton gardé pour la requête.
#[tauri::command]
pub fn oauth_clear(state: State<'_, AppState>, args: TokenArgs) -> Reply<()> {
    let (config, _, _) = resolve(&state, &args)?;
    if let Some(session) = state.sessions.lock().map_err(err)?.get_mut(&args.root) {
        session.tokens.remove(&token_key(&config));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Write};

    use tauri::Manager;

    use super::*;

    /// Un serveur qui répond par un jeton sur `/token` et renvoie sinon l'en-tête `Authorization` reçu ; chaque requête est lue
    /// en entier (en-têtes et corps).
    fn token_server() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    data.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&data).into_owned();
                    let Some((head, body)) = text.split_once("\r\n\r\n") else {
                        if n == 0 {
                            break;
                        }
                        continue;
                    };
                    let wanted = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if body.len() >= wanted || n == 0 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&data).into_owned();
                let body = if text.contains(" /token") {
                    r#"{"access_token":"tok","token_type":"Bearer","expires_in":3600,"refresh_token":"r"}"#.to_owned()
                } else {
                    let auth = text
                        .lines()
                        .find_map(|l| l.strip_prefix("authorization: ").or_else(|| l.strip_prefix("Authorization: ")))
                        .unwrap_or("");
                    format!(r#"{{"auth":"{auth}"}}"#)
                };
                write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).ok();
            }
        });
        base
    }

    fn setup(base: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("opencollection.yml"),
            format!("opencollection: 1.0.0\n\ninfo:\n  name: Sécurisée\n\nrequest:\n  variables:\n    - name: base\n      value: {base}\n"),
        )
        .unwrap();
        let oauth = "info:\n  name: Api\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/api\"\n  auth:\n    type: oauth2\n    flow: client_credentials\n    accessTokenUrl: \"{{base}}/token\"\n    credentials:\n      clientId: demo\n      clientSecret: s\n      placement: body\n";
        fs::write(dir.path().join("api.yml"), oauth).unwrap();
        fs::write(
            dir.path().join("plain.yml"),
            "info:\n  name: Plain\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{base}}/api\"\n",
        )
        .unwrap();
        let root = dir.path().display().to_string();
        (dir, root)
    }

    fn args(root: &str, path: &str) -> TokenArgs {
        TokenArgs {
            root: root.into(),
            path: path.into(),
            doc: xc_core::read_request(Path::new(root), path).unwrap(),
            env: None,
        }
    }

    #[tokio::test]
    async fn ef_aut_02_the_app_fetches_shows_and_clears_a_token_without_exposing_its_value() {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let state = || app.state::<AppState>();
        let (_dir, root) = setup(&token_server());

        assert_eq!(oauth_status(state(), args(&root, "api.yml")).unwrap(), None, "aucun jeton avant l'obtention");
        let info = oauth_fetch(app.handle().clone(), state(), args(&root, "api.yml")).await.unwrap();
        assert_eq!((info.id.as_str(), info.has_refresh_token, info.expired), ("credentials", true, false));
        assert!(
            !serde_json::to_string(&info).unwrap().contains("tok\""),
            "la valeur du jeton ne sort pas du processus"
        );

        assert_eq!(oauth_status(state(), args(&root, "api.yml")).unwrap(), Some(info));
        oauth_clear(state(), args(&root, "api.yml")).unwrap();
        assert_eq!(oauth_status(state(), args(&root, "api.yml")).unwrap(), None);
    }

    #[tokio::test]
    async fn ef_aut_02_a_request_without_oauth2_has_no_token_and_cannot_fetch_one() {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let state = || app.state::<AppState>();
        let (_dir, root) = setup("http://127.0.0.1:1");

        assert_eq!(oauth_status(state(), args(&root, "plain.yml")).unwrap(), None);
        let refused = oauth_fetch(app.handle().clone(), state(), args(&root, "plain.yml")).await.unwrap_err();
        assert!(refused.contains("n'utilise pas OAuth 2"), "{refused}");
    }

    #[tokio::test]
    async fn ef_aut_02_a_token_fetched_by_the_app_is_used_by_the_next_send() {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let state = || app.state::<AppState>();
        let (_dir, root) = setup(&token_server());

        oauth_fetch(app.handle().clone(), state(), args(&root, "api.yml")).await.unwrap();
        let session = state().sessions.lock().unwrap().get(&root).cloned().unwrap();
        assert_eq!(session.tokens.len(), 1);
        let sent = crate::send_request(
            app.handle().clone(),
            state(),
            crate::SendArgs {
                id: "a".into(),
                root: root.clone(),
                path: "api.yml".into(),
                doc: xc_core::read_request(Path::new(&root), "api.yml").unwrap(),
                env: None,
            },
        )
        .await
        .unwrap();
        let body = sent.response.map(|r| r.body).unwrap_or_default();
        assert!(body.contains("Bearer tok"), "{body}");
    }
}
