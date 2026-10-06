use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use base64::Engine as _;
use xc_core::{read_request, RequestDoc};
use xc_engine::aws::sha256;
use xc_runner::{run_request, Authorization, AuthorizationRequest, Authorizer, Outcome, Request, Session, Stage};

type Seen = Arc<Mutex<Vec<String>>>;

/// Un serveur qui garde chaque requête reçue en texte brut (corps compris) et répond en JSON avec `reply(requête)`.
fn serve(reply: impl Fn(&str) -> (u16, String) + Send + 'static) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap_or(0);
                data.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&data);
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
            let request = String::from_utf8_lossy(&data).into_owned();
            let (status, body) = reply(&request);
            log.lock().unwrap().push(request);
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (base, seen)
}

fn header(request: &str, name: &str) -> Option<String> {
    request.lines().take_while(|l| !l.is_empty()).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
    })
}

fn path_of(request: &str) -> &str {
    request.split_whitespace().nth(1).unwrap_or_default()
}

fn body_of(request: &str) -> HashMap<String, String> {
    let body = request.split_once("\r\n\r\n").map_or("", |(_, b)| b);
    form_urlencoded::parse(body.as_bytes()).map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}

fn collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("opencollection.yml"),
        format!("opencollection: 1.0.0\n\ninfo:\n  name: Secure\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n"),
    )
    .unwrap();
    dir
}

/// `auth` est le contenu de la table `oauth2`, sans retrait.
fn request(root: &Path, auth: &str) -> (String, RequestDoc) {
    let auth: String = auth.lines().map(|l| format!("    {l}\n")).collect();
    let text = format!(
        "info:\n  name: Api\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/api\"\n  headers:\n    - name: X-Token\n      value: \"{{{{$oauth2.credentials.access_token}}}}\"\n  auth:\n{auth}"
    );
    fs::write(root.join("api.yml"), text).unwrap();
    ("api.yml".to_owned(), read_request(root, "api.yml").unwrap())
}

async fn run(root: &Path, path: &str, doc: &RequestDoc, session: &mut Session) -> Outcome {
    let req = Request {
        root,
        path,
        doc,
        env: None,
        collection_name: "Secure",
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    run_request(req, session).await
}

fn token_reply(access: &str, extra: &str) -> (u16, String) {
    (200, format!(r#"{{"access_token":"{access}","token_type":"Bearer"{extra}}}"#))
}

fn count(seen: &Seen, path: &str) -> usize {
    seen.lock().unwrap().iter().filter(|r| path_of(r).starts_with(path)).count()
}

fn last(seen: &Seen, path: &str) -> String {
    seen.lock().unwrap().iter().rev().find(|r| path_of(r).starts_with(path)).cloned().unwrap()
}

const CC: &str = "type: oauth2\nflow: client_credentials\naccessTokenUrl: \"{{baseUrl}}/token\"\ncredentials:\n  clientId: demo\n  clientSecret: s3cret\n  placement: basic_auth_header\nscope: read write\ntokenConfig:\n  id: credentials\n  placement:\n    header: Bearer\n  source: access_token\nsettings:\n  autoFetchToken: true\n  autoRefreshToken: false";

#[tokio::test]
async fn ef_aut_02_client_credentials_fetches_a_token_once_and_reuses_it() {
    let (base, seen) = serve(|r| {
        if path_of(r).starts_with("/token") {
            token_reply("tok-1", r#","expires_in":3600"#)
        } else {
            (200, "{}".into())
        }
    });
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), CC);
    let mut session = Session::default();

    let first = run(dir.path(), &path, &doc, &mut session).await;
    assert!(first.error.is_none(), "{:?}", first.error);
    assert_eq!(first.response.as_ref().unwrap().status, 200);
    let token_request = last(&seen, "/token");
    let basic = base64::engine::general_purpose::STANDARD.encode("demo:s3cret");
    assert_eq!(header(&token_request, "authorization"), Some(format!("Basic {basic}")));
    assert!(token_request.starts_with("POST /token "), "{token_request}");
    let grant = body_of(&token_request);
    assert_eq!((grant["grant_type"].as_str(), grant["scope"].as_str()), ("client_credentials", "read write"));
    assert!(!grant.contains_key("client_secret"), "avec basic_auth_header, le secret ne va pas dans le corps");
    assert_eq!(header(&last(&seen, "/api"), "authorization"), Some("Bearer tok-1".into()));

    let second = run(dir.path(), &path, &doc, &mut session).await;
    assert!(second.error.is_none(), "{:?}", second.error);
    assert_eq!(count(&seen, "/token"), 1, "le jeton valide est gardé par la session");
    assert_eq!(count(&seen, "/api"), 2);
    assert_eq!(session.tokens.len(), 1);
}

#[tokio::test]
async fn ef_aut_02_the_token_is_readable_as_an_interpolated_variable() {
    let (base, seen) =
        serve(|r| if path_of(r).starts_with("/token") { token_reply("tok-var", "") } else { (200, "{}".into()) });
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), CC);
    let mut session = Session::default();

    run(dir.path(), &path, &doc, &mut session).await;
    // La première requête part avant que le jeton existe : la variable n'est pas encore résolue.
    run(dir.path(), &path, &doc, &mut session).await;
    assert_eq!(header(&last(&seen, "/api"), "x-token"), Some("tok-var".into()));
}

#[tokio::test]
async fn ef_aut_02_credentials_in_the_body_and_extra_parameters_are_sent() {
    let (base, seen) =
        serve(|r| if path_of(r).starts_with("/token") { token_reply("t", "") } else { (200, "{}".into()) });
    let dir = collection(&base);
    let auth = "type: oauth2\nflow: client_credentials\naccessTokenUrl: \"{{baseUrl}}/token?trace=1\"\ncredentials:\n  clientId: demo\n  clientSecret: s3cret\n  placement: body\nadditionalParameters:\n  accessTokenRequest:\n    - name: X-Tenant\n      value: acme\n      placement: header\n    - name: audience\n      value: api\n      placement: body";
    let (path, doc) = request(dir.path(), auth);
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    let token_request = last(&seen, "/token");
    assert!(token_request.starts_with("POST /token?trace=1 "), "{token_request}");
    assert_eq!(header(&token_request, "x-tenant"), Some("acme".into()));
    assert!(header(&token_request, "authorization").is_none());
    let grant = body_of(&token_request);
    assert_eq!(
        (grant["client_id"].as_str(), grant["client_secret"].as_str(), grant["audience"].as_str()),
        ("demo", "s3cret", "api")
    );
}

#[tokio::test]
async fn ef_aut_02_the_password_flow_sends_the_resource_owner() {
    let (base, seen) =
        serve(|r| if path_of(r).starts_with("/token") { token_reply("pw", "") } else { (200, "{}".into()) });
    let dir = collection(&base);
    let auth = "type: oauth2\nflow: resource_owner_password_credentials\naccessTokenUrl: \"{{baseUrl}}/token\"\ncredentials:\n  clientId: demo\n  placement: basic_auth_header\nresourceOwner:\n  username: alice\n  password: \"p&ss w0rd\"\ntokenConfig:\n  id: credentials\n  placement:\n    header: Token";
    let (path, doc) = request(dir.path(), auth);
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    let grant = body_of(&last(&seen, "/token"));
    assert_eq!(
        (grant["grant_type"].as_str(), grant["username"].as_str(), grant["password"].as_str()),
        ("password", "alice", "p&ss w0rd")
    );
    assert_eq!(
        header(&last(&seen, "/api"), "authorization"),
        Some("Token pw".into()),
        "le préfixe de l'en-tête est celui de la configuration"
    );
}

#[tokio::test]
async fn ef_aut_02_an_expired_token_is_refreshed_when_auto_refresh_is_on() {
    let (base, seen) = serve(|r| {
        if !path_of(r).starts_with("/token") {
            return (200, "{}".into());
        }
        match body_of(r).get("grant_type").map(String::as_str) {
            Some("refresh_token") => token_reply("fresh", r#","expires_in":3600"#),
            _ => token_reply("stale", r#","expires_in":1,"refresh_token":"r-1""#),
        }
    });
    let dir = collection(&base);
    let auth = CC.replace("autoRefreshToken: false", "autoRefreshToken: true");
    let (path, doc) = request(dir.path(), &auth);
    let mut session = Session::default();

    run(dir.path(), &path, &doc, &mut session).await;
    assert_eq!(header(&last(&seen, "/api"), "authorization"), Some("Bearer stale".into()));
    run(dir.path(), &path, &doc, &mut session).await;

    let refresh = body_of(&last(&seen, "/token"));
    assert_eq!((refresh["grant_type"].as_str(), refresh["refresh_token"].as_str()), ("refresh_token", "r-1"));
    assert_eq!(header(&last(&seen, "/api"), "authorization"), Some("Bearer fresh".into()));
    let token = session.tokens.values().next().unwrap();
    assert_eq!(
        token.refresh_token.as_deref(),
        Some("r-1"),
        "le jeton de rafraîchissement est gardé quand le serveur n'en renvoie pas"
    );
}

#[tokio::test]
async fn ef_aut_02_an_expired_token_is_fetched_again_without_auto_refresh() {
    let hits = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&hits);
    let (base, seen) = serve(move |r| {
        if !path_of(r).starts_with("/token") {
            return (200, "{}".into());
        }
        let mut n = counter.lock().unwrap();
        *n += 1;
        token_reply(&format!("tok-{n}"), r#","expires_in":1,"refresh_token":"r""#)
    });
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), CC);
    let mut session = Session::default();

    run(dir.path(), &path, &doc, &mut session).await;
    run(dir.path(), &path, &doc, &mut session).await;
    assert_eq!(count(&seen, "/token"), 2);
    assert_eq!(body_of(&last(&seen, "/token"))["grant_type"], "client_credentials");
    assert_eq!(header(&last(&seen, "/api"), "authorization"), Some("Bearer tok-2".into()));
}

#[tokio::test]
async fn ef_aut_02_without_auto_fetch_no_token_is_requested() {
    let (base, seen) = serve(|_| (200, "{}".into()));
    let dir = collection(&base);
    let auth = CC.replace("autoFetchToken: true", "autoFetchToken: false");
    let (path, doc) = request(dir.path(), &auth);
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(count(&seen, "/token"), 0);
    assert!(header(&last(&seen, "/api"), "authorization").is_none());
}

#[tokio::test]
async fn ef_aut_02_the_token_can_travel_in_the_query_string() {
    let (base, seen) =
        serve(|r| if path_of(r).starts_with("/token") { token_reply("q-tok", "") } else { (200, "{}".into()) });
    let dir = collection(&base);
    let auth = "type: oauth2\nflow: client_credentials\naccessTokenUrl: \"{{baseUrl}}/token\"\ncredentials:\n  clientId: demo\n  placement: body\ntokenConfig:\n  id: credentials\n  placement:\n    query: key";
    let (path, doc) = request(dir.path(), auth);
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert!(last(&seen, "/api").starts_with("GET /api?key=q-tok "), "{}", last(&seen, "/api"));
    assert!(header(&last(&seen, "/api"), "authorization").is_none());
}

#[tokio::test]
async fn ef_aut_02_a_refusal_from_the_token_endpoint_fails_the_request_with_its_reason() {
    let (base, seen) =
        serve(|_| (400, r#"{"error":"invalid_client","error_description":"Client authentication failed"}"#.into()));
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), CC);
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;

    assert_eq!(out.error.as_ref().map(|e| e.stage), Some(Stage::Send));
    let message = out.error.unwrap().message;
    assert!(message.contains("invalid_client : Client authentication failed"), "{message}");
    assert_eq!(count(&seen, "/api"), 0, "la requête n'est pas envoyée sans jeton");
}

#[tokio::test]
async fn ef_aut_02_a_response_without_access_token_is_an_error() {
    let (base, _) = serve(|_| (200, r#"{"hello":"world"}"#.into()));
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), CC);
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;

    assert!(out.error.unwrap().message.contains("access_token"));
}

type Respond = Box<dyn Fn(&AuthorizationRequest) -> Result<HashMap<String, String>, String> + Send + Sync>;

struct Fake {
    requests: Mutex<Vec<AuthorizationRequest>>,
    respond: Respond,
}

impl std::fmt::Debug for Fake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fake").finish_non_exhaustive()
    }
}

impl Fake {
    fn new(
        respond: impl Fn(&AuthorizationRequest) -> Result<HashMap<String, String>, String> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self { requests: Mutex::default(), respond: Box::new(respond) })
    }
}

impl Authorizer for Fake {
    fn authorize(&self, request: AuthorizationRequest) -> Authorization<'_> {
        let result = (self.respond)(&request);
        self.requests.lock().unwrap().push(request);
        Box::pin(async move { result })
    }
}

fn query_of(url: &str) -> HashMap<String, String> {
    url::Url::parse(url).unwrap().query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}

fn code_reply(request: &AuthorizationRequest) -> Result<HashMap<String, String>, String> {
    let query = query_of(&request.url);
    Ok(HashMap::from([("code".to_owned(), "the-code".to_owned()), ("state".to_owned(), query["state"].clone())]))
}

const CODE: &str = "type: oauth2\nflow: authorization_code\nauthorizationUrl: \"{{baseUrl}}/authorize?tenant=acme\"\naccessTokenUrl: \"{{baseUrl}}/token\"\ncallbackUrl: http://localhost/cb\ncredentials:\n  clientId: demo\n  clientSecret: s3cret\n  placement: body\nscope: openid\npkce: {}\ntokenConfig:\n  id: credentials\n  placement:\n    header: Bearer";

#[tokio::test]
async fn ef_aut_02_authorization_code_with_pkce_proves_the_verifier_to_the_token_endpoint() {
    let challenge = Arc::new(Mutex::new(String::new()));
    let expected = Arc::clone(&challenge);
    let (base, seen) = serve(move |r| {
        if !path_of(r).starts_with("/token") {
            return (200, "{}".into());
        }
        let grant = body_of(r);
        let proof = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha256(grant["code_verifier"].as_bytes()));
        if grant["code"] == "the-code" && proof == *expected.lock().unwrap() {
            token_reply("code-tok", "")
        } else {
            (400, r#"{"error":"invalid_grant"}"#.into())
        }
    });
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), CODE);
    let remember = Arc::clone(&challenge);
    let fake = Fake::new(move |request| {
        *remember.lock().unwrap() = query_of(&request.url)["code_challenge"].clone();
        code_reply(request)
    });
    let mut session = Session { authorizer: Some(fake.clone()), ..Session::default() };
    let out = run(dir.path(), &path, &doc, &mut session).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(header(&last(&seen, "/api"), "authorization"), Some("Bearer code-tok".into()));
    let requests = fake.requests.lock().unwrap();
    let query = query_of(&requests[0].url);
    assert_eq!(requests[0].callback_url, "http://localhost/cb");
    assert!(requests[0].url.starts_with(&format!("{base}/authorize?tenant=acme&")), "{}", requests[0].url);
    assert_eq!(
        (
            query["response_type"].as_str(),
            query["client_id"].as_str(),
            query["redirect_uri"].as_str(),
            query["scope"].as_str(),
            query["code_challenge_method"].as_str()
        ),
        ("code", "demo", "http://localhost/cb", "openid", "S256")
    );
    let grant = body_of(&last(&seen, "/token"));
    assert_eq!(
        (grant["grant_type"].as_str(), grant["redirect_uri"].as_str(), grant["client_secret"].as_str()),
        ("authorization_code", "http://localhost/cb", "s3cret")
    );
}

#[tokio::test]
async fn ef_aut_02_authorization_code_without_pkce_sends_no_challenge() {
    let (base, seen) =
        serve(|r| if path_of(r).starts_with("/token") { token_reply("t", "") } else { (200, "{}".into()) });
    let dir = collection(&base);
    let auth = CODE.replace("pkce: {}\n", "");
    let (path, doc) = request(dir.path(), &auth);
    let fake = Fake::new(code_reply);
    let mut session = Session { authorizer: Some(fake.clone()), ..Session::default() };
    let out = run(dir.path(), &path, &doc, &mut session).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert!(!fake.requests.lock().unwrap()[0].url.contains("code_challenge"));
    assert!(!body_of(&last(&seen, "/token")).contains_key("code_verifier"));
}

#[tokio::test]
async fn ef_aut_02_a_mismatched_state_is_rejected() {
    let (base, seen) = serve(|_| (200, "{}".into()));
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), CODE);
    let fake = Fake::new(|_| {
        Ok(HashMap::from([("code".to_owned(), "c".to_owned()), ("state".to_owned(), "forged".to_owned())]))
    });
    let mut session = Session { authorizer: Some(fake), ..Session::default() };
    let out = run(dir.path(), &path, &doc, &mut session).await;

    assert!(out.error.unwrap().message.contains("state"));
    assert_eq!(count(&seen, "/token"), 0);
}

#[tokio::test]
async fn ef_aut_02_a_denied_authorization_reports_the_servers_error() {
    let (base, _) = serve(|_| (200, "{}".into()));
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), CODE);
    let fake = Fake::new(|_| {
        Ok(HashMap::from([
            ("error".to_owned(), "access_denied".to_owned()),
            ("error_description".to_owned(), "no thanks".to_owned()),
        ]))
    });
    let mut session = Session { authorizer: Some(fake), ..Session::default() };
    let out = run(dir.path(), &path, &doc, &mut session).await;

    assert!(out.error.unwrap().message.contains("access_denied : no thanks"));
}

#[tokio::test]
async fn ef_aut_02_an_interactive_flow_without_a_host_explains_what_is_missing() {
    let (base, _) = serve(|_| (200, "{}".into()));
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), CODE);
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;

    let message = out.error.unwrap().message;
    assert!(message.contains("fenêtre de connexion") && message.contains("authorization_code"), "{message}");
}

#[tokio::test]
async fn ef_aut_02_the_implicit_flow_takes_the_token_from_the_redirect() {
    let (base, seen) = serve(|_| (200, "{}".into()));
    let dir = collection(&base);
    let auth = "type: oauth2\nflow: implicit\nauthorizationUrl: \"{{baseUrl}}/authorize\"\ncallbackUrl: http://localhost/cb\ncredentials:\n  clientId: demo\ntokenConfig:\n  id: credentials\n  placement:\n    header: Bearer";
    let (path, doc) = request(dir.path(), auth);
    let fake = Fake::new(|request| {
        let state = query_of(&request.url)["state"].clone();
        Ok(HashMap::from([
            ("access_token".to_owned(), "implicit-tok".to_owned()),
            ("state".to_owned(), state),
            ("expires_in".to_owned(), "3600".to_owned()),
        ]))
    });
    let mut session = Session { authorizer: Some(fake.clone()), ..Session::default() };
    let out = run(dir.path(), &path, &doc, &mut session).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(query_of(&fake.requests.lock().unwrap()[0].url)["response_type"], "token");
    assert_eq!(header(&last(&seen, "/api"), "authorization"), Some("Bearer implicit-tok".into()));
    assert_eq!(count(&seen, "/token"), 0, "le flux implicite n'appelle pas le point d'accès aux jetons");
    assert!(session.tokens.values().next().unwrap().expires_at.is_some());
}

#[tokio::test]
async fn ef_aut_02_the_id_token_can_replace_the_access_token() {
    let (base, seen) = serve(|r| {
        if path_of(r).starts_with("/token") {
            token_reply("acc", r#","id_token":"idt""#)
        } else {
            (200, "{}".into())
        }
    });
    let dir = collection(&base);
    let auth = format!("{CC}\n").replace("source: access_token", "source: id_token");
    let (path, doc) = request(dir.path(), auth.trim_end());
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(header(&last(&seen, "/api"), "authorization"), Some("Bearer idt".into()));
}

#[tokio::test]
async fn ef_aut_02_a_test_script_reads_the_token_obtained_at_send_time() {
    let (base, _) = serve(|r| {
        if path_of(r).starts_with("/token") {
            token_reply("tok-script", r#","custom":"x""#)
        } else {
            (200, "{}".into())
        }
    });
    let dir = collection(&base);
    let (path, _) = request(dir.path(), CC);
    let text = fs::read_to_string(dir.path().join(&path)).unwrap()
        + "\nruntime:\n  scripts:\n    - type: tests\n      code: |-\n        test('token', () => expect(bru.getOauth2CredentialVar('$oauth2.credentials.access_token')).to.equal('tok-script'));\n        test('champ libre', () => expect(bru.interpolate('{{$oauth2.credentials.custom}}')).to.equal('x'));\n";
    fs::write(dir.path().join(&path), text).unwrap();
    let doc = read_request(dir.path(), &path).unwrap();
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    let statuses: Vec<_> = out.tests.results.iter().map(|r| (r.description.as_str(), r.status.as_str())).collect();
    assert_eq!(statuses, [("token", "pass"), ("champ libre", "pass")], "{:?}", out.tests);
}
