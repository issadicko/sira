use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use xc_sync::curl::{curl_to_json, parse_curl_command, request_from_curl, request_from_curl_typed};

fn parsed(command: &str) -> Value {
    parse_curl_command(command).unwrap().to_json()
}

fn assert_parsed(command: &str, expected: Value) {
    assert_eq!(parsed(command), expected, "{command}");
}

fn to_json(command: &str) -> Value {
    curl_to_json(command).unwrap().unwrap().to_json()
}

fn get(url: &str) -> Value {
    json!({ "method": "get", "url": url, "urlWithoutQuery": url })
}

fn with(mut base: Value, extra: Value) -> Value {
    base.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    base
}

const COMPLEX_DATA: &str = r#"{"name": "John's data", "email": "john@example.com", "message": "Don't stop believing!", "path": "/home/user/file.txt", "json": {"nested": "value", "array": [1, 2, 3]}}"#;

#[test]
fn ef_imp_01_parse_simple_get() {
    assert_parsed("\n        curl https://api.example.com/users\n      ", get("https://api.example.com/users"));
}

#[test]
fn ef_imp_01_parse_explicit_post() {
    assert_parsed(
        "curl -X POST https://api.example.com/users",
        with(get("https://api.example.com/users"), json!({ "method": "post" })),
    );
}

#[test]
fn ef_imp_01_parse_put_method() {
    assert_parsed(
        "curl -X PUT https://api.example.com/users/1",
        with(get("https://api.example.com/users/1"), json!({ "method": "put" })),
    );
}

#[test]
fn ef_imp_01_parse_delete_method() {
    assert_parsed(
        "curl -X DELETE https://api.example.com/users/1",
        with(get("https://api.example.com/users/1"), json!({ "method": "delete" })),
    );
}

#[test]
fn ef_imp_01_parse_head_method() {
    assert_parsed(
        "curl -I https://api.example.com/users",
        with(get("https://api.example.com/users"), json!({ "method": "head" })),
    );
}

#[test]
fn ef_imp_01_parse_single_header() {
    let expected = with(get("https://api.example.com"), json!({ "headers": { "Content-Type": "application/json" } }));
    assert_parsed(r#"curl --header "Content-Type: application/json" https://api.example.com"#, expected.clone());
    assert_parsed(r#"curl --header "Content-Type:application/json" https://api.example.com"#, expected);
}

#[test]
fn ef_imp_01_parse_multiple_headers() {
    assert_parsed(
        "curl -H \"Content-Type: application/json\" \
             -H \"Authorization: Bearer token\" \
             https://api.example.com",
        with(
            get("https://api.example.com"),
            json!({ "headers": { "Content-Type": "application/json", "Authorization": "Bearer token" } }),
        ),
    );
}

#[test]
fn ef_imp_01_parse_user_agent() {
    assert_parsed(
        r#"curl -A "Custom User Agent" https://api.example.com"#,
        with(get("https://api.example.com"), json!({ "headers": { "User-Agent": "Custom User Agent" } })),
    );
}

#[test]
fn ef_imp_01_parse_json_data_switches_to_post() {
    assert_parsed(
        r#"curl -d '{"name": "John", "age": 30}' https://api.example.com/users"#,
        with(
            get("https://api.example.com/users"),
            json!({ "method": "post", "data": r#"{"name": "John", "age": 30}"# }),
        ),
    );
}

#[test]
fn ef_imp_01_parse_post_data_and_multiple_data_flags() {
    let expected = with(get("https://api.example.com/users"), json!({ "method": "post", "data": "name=John&age=30" }));
    assert_parsed(r#"curl --data "name=John&age=30" https://api.example.com/users"#, expected.clone());
    assert_parsed(
        "curl -d \"name=John\" \
             -d \"age=30\" \
             https://api.example.com/users",
        expected,
    );
}

#[test]
fn ef_imp_01_parse_keeps_multiline_and_multi_space_data() {
    let multiline = "{\"key\": \"some long message with line breaks\n\n\n             multiline\"}";
    assert_parsed(
        &format!("\n        curl -d '{multiline}'              https://api.example.com/users\n      "),
        with(get("https://api.example.com/users"), json!({ "method": "post", "data": multiline })),
    );
    let spaced = r#"{"key": "some long    spaced     message"}"#;
    assert_parsed(
        &format!("curl -d '{spaced}'              https://api.example.com/users"),
        with(get("https://api.example.com/users"), json!({ "method": "post", "data": spaced })),
    );
}

#[test]
fn ef_imp_01_parse_binary_and_raw_data_flags() {
    assert_parsed(
        r#"curl --data-binary "@/path/to/file" https://api.example.com/upload"#,
        with(
            get("https://api.example.com/upload"),
            json!({ "method": "post", "data": "@/path/to/file", "isDataBinary": true }),
        ),
    );
    assert_parsed(
        r#"curl --data-binary '{"pageUri": "/mobile-phones-store"}' https://api.example.com/page/fetch"#,
        with(
            get("https://api.example.com/page/fetch"),
            json!({ "method": "post", "data": r#"{"pageUri": "/mobile-phones-store"}"#, "isDataBinary": true }),
        ),
    );
    assert_parsed(
        r#"curl --data-raw '{"raw": "data"}' https://api.example.com"#,
        with(
            get("https://api.example.com"),
            json!({ "method": "post", "data": r#"{"raw": "data"}"#, "isDataRaw": true }),
        ),
    );
}

#[test]
fn ef_imp_01_parse_basic_auth() {
    let auth = |username: &str, password: &str| {
        with(
            get("https://api.example.com"),
            json!({ "auth": { "mode": "basic", "basic": { "username": username, "password": password } } }),
        )
    };
    assert_parsed(r#"curl -u "username:password" https://api.example.com"#, auth("username", "password"));
    assert_parsed(r#"curl --user "username" https://api.example.com"#, auth("username", ""));
}

#[test]
fn ef_imp_01_parse_digest_and_ntlm_auth() {
    let auth = |url: &str, mode: &str, username: &str, password: &str| {
        with(get(url), json!({ "auth": { "mode": mode, mode: { "username": username, "password": password } } }))
    };
    assert_parsed(
        r#"curl --digest -u "myuser:mypass" https://api.example.com/digest"#,
        auth("https://api.example.com/digest", "digest", "myuser", "mypass"),
    );
    assert_parsed(
        r#"curl --digest --user "admin:secret" https://api.example.com/secure"#,
        auth("https://api.example.com/secure", "digest", "admin", "secret"),
    );
    assert_parsed(
        r#"curl --ntlm -u "myuser:mypass" https://api.example.com/ntlm"#,
        auth("https://api.example.com/ntlm", "ntlm", "myuser", "mypass"),
    );
    assert_parsed(
        r#"curl --ntlm --user "domain\username:password" https://api.example.com/ntlm"#,
        auth("https://api.example.com/ntlm", "ntlm", "domain\\username", "password"),
    );
    assert_parsed(
        r#"curl -u "user:pass" --digest https://api.example.com"#,
        auth("https://api.example.com", "digest", "user", "pass"),
    );
}

#[test]
fn ef_imp_01_parse_form_data() {
    assert_parsed(
        "curl -F \"name=John\" \
             -F \"age=30\" \
             https://api.example.com/users",
        with(
            get("https://api.example.com/users"),
            json!({ "method": "post", "multipartUploads": [
                { "name": "name", "value": "John", "type": "text", "enabled": true },
                { "name": "age", "value": "30", "type": "text", "enabled": true }
            ] }),
        ),
    );
    assert_parsed(
        r#"curl --form "file=@/path/to/file.txt" https://api.example.com/upload"#,
        with(
            get("https://api.example.com/upload"),
            json!({ "method": "post", "multipartUploads": [
                { "name": "file", "value": "/path/to/file.txt", "type": "file", "enabled": true }
            ] }),
        ),
    );
}

fn cookie_request(cookie_string: &str, cookies: Value) -> Value {
    with(
        get("https://api.example.com"),
        json!({ "headers": { "Cookie": cookie_string }, "cookieString": cookie_string, "cookies": cookies }),
    )
}

#[test]
fn ef_imp_01_parse_cookies() {
    assert_parsed(
        r#"curl -b "session=abc123" https://api.example.com"#,
        cookie_request("session=abc123", json!({ "session": "abc123" })),
    );
    let both = cookie_request("session=abc123; user=john", json!({ "session": "abc123", "user": "john" }));
    assert_parsed(r#"curl -b "session=abc123; user=john" https://api.example.com"#, both.clone());
    assert_parsed(r#"curl -b "session=abc123" -b "user=john" https://api.example.com"#, both);
}

#[test]
fn ef_imp_01_parse_complex_cookie_string() {
    let cookie = "session=abc123; user=john; path=/; domain=example.com; expires=Thu, 01 Jan 1970 00:00:00 GMT; secure; HttpOnly";
    assert_parsed(
        &format!("curl -b \"{cookie}\"              https://api.example.com"),
        cookie_request(
            cookie,
            json!({ "session": "abc123", "user": "john", "path": "/", "domain": "example.com",
                    "expires": "Thu, 01 Jan 1970 00:00:00 GMT" }),
        ),
    );
}

#[test]
fn ef_imp_01_parse_shell_quote_patterns() {
    assert_parsed(
        r#"curl -d '{"name": "John'\''s data"}' https://api.example.com"#,
        with(get("https://api.example.com"), json!({ "method": "post", "data": r#"{"name": "John's data"}"# })),
    );
    assert_parsed(
        r#"curl -d '{"message": "Don\'t stop believing"}' https://api.example.com"#,
        with(
            get("https://api.example.com"),
            json!({ "method": "post", "data": r#"{"message": "Don't stop believing"}"# }),
        ),
    );
}

#[test]
fn ef_imp_01_parse_urls_with_query_and_paths() {
    assert_parsed(
        "curl https://api.example.com/users?page=1&limit=10&sort=asc",
        json!({
            "method": "get",
            "queries": [
                { "name": "page", "value": "1" }, { "name": "limit", "value": "10" }, { "name": "sort", "value": "asc" }
            ],
            "url": "https://api.example.com/users?page=1&limit=10&sort=asc",
            "urlWithoutQuery": "https://api.example.com/users"
        }),
    );
    assert_parsed("curl https://api.example.com/v1/users/123", get("https://api.example.com/v1/users/123"));
}

#[test]
fn ef_imp_01_parse_urls_without_protocol() {
    assert_parsed("curl echo.usebruno.com", get("https://echo.usebruno.com"));
    assert_parsed(
        "curl api.example.com/users?page=1&limit=10",
        json!({
            "method": "get",
            "url": "https://api.example.com/users?page=1&limit=10",
            "urlWithoutQuery": "https://api.example.com/users",
            "queries": [{ "name": "page", "value": "1" }, { "name": "limit", "value": "10" }]
        }),
    );
}

fn complex_expected(url: &str, url_without_query: &str, queries: Option<Value>) -> Value {
    let mut expected = json!({
        "method": "post",
        "headers": {
            "Content-Type": "application/json",
            "Authorization": "Bearer token123",
            "X-Custom-Header": "custom header",
            "Accept-Encoding": "deflate, gzip"
        },
        "data": COMPLEX_DATA,
        "auth": { "mode": "basic", "basic": { "username": "api_user", "password": "api_pass" } },
        "url": url,
        "urlWithoutQuery": url_without_query
    });
    if let Some(queries) = queries {
        expected["queries"] = queries;
    }
    expected
}

fn complex_command(url: &str) -> String {
    format!(
        "curl -X POST \
             -H \"Content-Type: application/json\" \
             -H \"Authorization: Bearer token123\" \
             -H \"X-Custom-Header: custom header\" \
             -d '{}' \
             -u \"api_user:api_pass\" \
             --compressed \
             {url}",
        r#"{"name": "John\'s data", "email": "john@example.com", "message": "Don\'t stop believing!", "path": "/home/user/file.txt", "json": {"nested": "value", "array": [1, 2, 3]}}"#
    )
}

#[test]
fn ef_imp_01_parse_complex_commands() {
    let queries = json!([{ "name": "param1", "value": "value1" }, { "name": "param2", "value": "custom+param" }]);
    let with_query = "https://api.example.com/v1/users?param1=value1&param2=custom+param";
    assert_parsed(
        &complex_command("api.example.com/v1/users?param1=value1&param2=custom+param"),
        complex_expected(with_query, "https://api.example.com/v1/users", Some(queries.clone())),
    );
    assert_parsed(
        &complex_command(with_query),
        complex_expected(with_query, "https://api.example.com/v1/users", Some(queries)),
    );
}

#[test]
fn ef_imp_01_parse_complex_escape_characters() {
    assert_parsed(
        "curl -X POST \
             -H \"Content-Type: application/json\" \
             -H \"Authorization: Bearer token123\" \
             -d '{\"name\": \"John\\'s data\", \"email\": \"john@example.com\"}' \
             -u \"api_user:api_pass\" \
             --compressed \
             https://api.example.com/v1/users",
        json!({
            "method": "post",
            "headers": {
                "Content-Type": "application/json",
                "Authorization": "Bearer token123",
                "Accept-Encoding": "deflate, gzip"
            },
            "data": r#"{"name": "John's data", "email": "john@example.com"}"#,
            "auth": { "mode": "basic", "basic": { "username": "api_user", "password": "api_pass" } },
            "url": "https://api.example.com/v1/users",
            "urlWithoutQuery": "https://api.example.com/v1/users"
        }),
    );
}

#[test]
fn ef_imp_01_parse_edge_cases() {
    assert_parsed(
        "curl --compressed https://api.example.com",
        with(get("https://api.example.com"), json!({ "headers": { "Accept-Encoding": "deflate, gzip" } })),
    );
    assert_parsed(
        "\n        curl -XPOST https://api.example.com/users\n      ",
        with(get("https://api.example.com/users"), json!({ "method": "post" })),
    );
    assert_parsed(
        "curl -H \"Content-Type: application/json\" \
             -d '{\"name\": \"John\"}' \
             https://api.example.com/users",
        with(
            get("https://api.example.com/users"),
            json!({ "method": "post", "headers": { "Content-Type": "application/json" }, "data": r#"{"name": "John"}"# }),
        ),
    );
}

fn json_flag(data: &str, url: &str, method: &str, extra_headers: Value) -> Value {
    let mut headers = json!({ "Content-Type": "application/json" });
    headers.as_object_mut().unwrap().extend(extra_headers.as_object().unwrap().clone());
    with(get(url), json!({ "method": method, "headers": headers, "data": data }))
}

#[test]
fn ef_imp_01_parse_json_flag() {
    let users = "https://api.example.com/users";
    let basic = r#"{"name": "John Doe", "email": "john@example.com"}"#;
    assert_parsed(&format!("curl --json '{basic}' {users}"), json_flag(basic, users, "post", json!({})));
    let post = r#"{"title": "New Post", "content": "Post content"}"#;
    assert_parsed(
        &format!(r#"curl --json '{post}' -H "Authorization: Bearer token123" https://api.example.com/posts"#),
        json_flag(post, "https://api.example.com/posts", "post", json!({ "Authorization": "Bearer token123" })),
    );
    let nested = r#"{"user": {"name": "Jane", "email": "jane@example.com"}, "metadata": {"source": "web"}}"#;
    assert_parsed(&format!("curl --json '{nested}' {users}"), json_flag(nested, users, "post", json!({})));
    assert_parsed(
        r#"curl --json '{"message": "Don\'t stop believing!", "user": "John\'s account"}' https://api.example.com/messages"#,
        json_flag(
            r#"{"message": "Don't stop believing!", "user": "John's account"}"#,
            "https://api.example.com/messages",
            "post",
            json!({}),
        ),
    );
    let items = r#"{"items": [{"id": 1, "name": "Item 1"}, {"id": 2, "name": "Item 2"}], "total": 2}"#;
    assert_parsed(
        &format!("curl --json '{items}' https://api.example.com/orders"),
        json_flag(items, "https://api.example.com/orders", "post", json!({})),
    );
    let task = r#"{"status": "completed", "updated_at": "2024-01-15T10:30:00Z"}"#;
    assert_parsed(
        &format!("curl -X PUT --json '{task}' https://api.example.com/tasks/123"),
        json_flag(task, "https://api.example.com/tasks/123", "put", json!({})),
    );
}

#[test]
fn ef_imp_01_parse_insecure_flag() {
    let expected = with(get("https://api.example.com"), json!({ "insecure": true }));
    assert_parsed("curl -k https://api.example.com", expected.clone());
    assert_parsed("curl --insecure https://api.example.com", expected);
}

#[test]
fn ef_imp_01_parse_get_flag_moves_data_to_query() {
    assert_parsed(
        r#"curl -G -d "name=John" -d "age=30" https://api.example.com/users"#,
        json!({
            "method": "get",
            "url": "https://api.example.com/users?name=John&age=30",
            "urlWithoutQuery": "https://api.example.com/users",
            "queries": [{ "name": "name", "value": "John" }, { "name": "age", "value": "30" }]
        }),
    );
    assert_parsed(
        "curl -G --data-urlencode \"name=John Doe\" \
             --data-urlencode \"email=john@example.com\" \
             --data-urlencode \"hello\" \
             https://api.example.com/users?test=urlquery&hello",
        json!({
            "method": "get",
            "url": "https://api.example.com/users?test=urlquery&name=John%20Doe&email=john@example.com&hello",
            "urlWithoutQuery": "https://api.example.com/users",
            "queries": [
                { "name": "test", "value": "urlquery" },
                { "name": "name", "value": "John%20Doe" },
                { "name": "email", "value": "john@example.com" },
                { "name": "hello" }
            ]
        }),
    );
    assert_parsed(
        "curl -G -d \"search=test+query\" \
             -d \"filter=active\" \
             -d \"sort=name\" \
             -d \"page=1\" \
             https://api.example.com/search",
        json!({
            "method": "get",
            "url": "https://api.example.com/search?search=test+query&filter=active&sort=name&page=1",
            "urlWithoutQuery": "https://api.example.com/search",
            "queries": [
                { "name": "search", "value": "test+query" },
                { "name": "filter", "value": "active" },
                { "name": "sort", "value": "name" },
                { "name": "page", "value": "1" }
            ]
        }),
    );
}

#[test]
fn ef_imp_01_curl_to_json_simple_and_trailing_slash() {
    assert_eq!(
        to_json("curl https://www.usebruno.com"),
        json!({ "url": "https://www.usebruno.com", "raw_url": "https://www.usebruno.com", "method": "get" })
    );
    assert_eq!(
        to_json("curl https://www.usebruno.com/"),
        json!({ "url": "https://www.usebruno.com/", "raw_url": "https://www.usebruno.com/", "method": "get" })
    );
}

#[test]
fn ef_imp_01_curl_to_json_headers() {
    let command = "curl https://www.usebruno.com
    -H 'Accept: application/json, text/plain, */*'
    -H 'Accept-Language: en-US,en;q=0.9,hi;q=0.8'
    ";
    assert_eq!(
        to_json(command),
        json!({
            "url": "https://www.usebruno.com",
            "raw_url": "https://www.usebruno.com",
            "method": "get",
            "headers": { "Accept": "application/json, text/plain, */*", "Accept-Language": "en-US,en;q=0.9,hi;q=0.8" }
        })
    );
}

const BROWSER_HEADERS: &str = "curl 'https://www.usebruno.com'
    -H 'Accept: application/json, text/plain, */*'
    -H 'Accept-Language: en-US,en;q=0.9,hi;q=0.8'
    -H 'Content-Type: application/json;charset=utf-8'
    -H 'Origin: https://www.usebruno.com'
    -H 'Referer: https://www.usebruno.com/'";

fn browser_headers() -> Value {
    json!({
        "Accept": "application/json, text/plain, */*",
        "Accept-Language": "en-US,en;q=0.9,hi;q=0.8",
        "Content-Type": "application/json;charset=utf-8",
        "Origin": "https://www.usebruno.com",
        "Referer": "https://www.usebruno.com/"
    })
}

#[test]
fn ef_imp_01_curl_to_json_post_body() {
    let command =
        format!("{BROWSER_HEADERS}\n    --data-raw '{{\"email\":\"test@usebruno.com\",\"password\":\"test\"}}'\n    ");
    assert_eq!(
        to_json(&command),
        json!({
            "url": "https://www.usebruno.com",
            "raw_url": "https://www.usebruno.com",
            "method": "post",
            "headers": browser_headers(),
            "data": r#"{"email":"test@usebruno.com","password":"test"}"#
        })
    );
}

#[test]
fn ef_imp_01_curl_to_json_binary_file_body() {
    let command = format!("{BROWSER_HEADERS}\n    --data-binary '@/path/to/file'\n    ");
    assert_eq!(
        to_json(&command),
        json!({
            "url": "https://www.usebruno.com",
            "raw_url": "https://www.usebruno.com",
            "method": "post",
            "headers": browser_headers(),
            "isDataBinary": true,
            "data": [{ "filePath": "/path/to/file", "contentType": "application/json;charset=utf-8", "selected": true }]
        })
    );
}

#[test]
fn ef_imp_01_curl_to_json_escaped_ansi_c_string() {
    let command = "curl https://www.usebruno.com
    -H $'cookie: val_1=\\'\\'; val_2=\\^373:0\\^373:0; val_3=hello'
    ";
    assert_eq!(
        to_json(command),
        json!({
            "url": "https://www.usebruno.com",
            "raw_url": "https://www.usebruno.com",
            "method": "get",
            "headers": { "cookie": "val_1=''; val_2=\\^373:0\\^373:0; val_3=hello" }
        })
    );
}

#[test]
fn ef_imp_01_curl_to_json_custom_json_content_types() {
    let command = "curl 'https://api.example.com/test'
    -H 'content-type: application/x.custom+json;version=1'
    --data-raw '{\"test\":\"data\"}'
    ";
    assert_eq!(
        to_json(command),
        json!({
            "url": "https://api.example.com/test",
            "raw_url": "https://api.example.com/test",
            "method": "post",
            "headers": { "content-type": "application/x.custom+json;version=1" },
            "data": r#"{"test":"data"}"#
        })
    );
    let vendor = r#"curl --request POST \
      --url https://api.example.com/orders/42/preferences \
      --header 'accept: */*' \
      --header 'content-type: application/vnd.vendor+json' \
      --data '{\n  "data": {\n    "type": "order-preferences",\n    "attributes": {\n      "notes": "Leave at door",\n      "priority": true\n    }\n  }\n}'"#;
    let result = to_json(vendor);
    let data = result["data"].as_str().unwrap();
    for part in [r#""type": "order-preferences""#, r#""notes": "Leave at door""#, r#""priority": true"#] {
        assert!(data.contains(part), "{data}");
    }
    assert_eq!(result["headers"]["content-type"], "application/vnd.vendor+json");
}

#[test]
fn ef_imp_01_inline_binary_json_is_a_json_body() {
    let request = request_from_curl(
        r#"curl -H "Content-Type: application/json; charset=UTF-8" --data-binary "{\"pageUri\":\"/mobile-phones-store\"}" "https://1.rome.api.flipkart.net/4/page/fetch""#,
    )
    .unwrap();
    assert_eq!(request["body"]["mode"], "json");
    assert!(request["body"]["file"].is_null());
    let body: Value = serde_json::from_str(request["body"]["json"].as_str().unwrap()).unwrap();
    assert_eq!(body, json!({ "pageUri": "/mobile-phones-store" }));
}

#[test]
fn ef_imp_01_binary_file_reference_is_a_file_body() {
    let request = request_from_curl(
        r#"curl -H "Content-Type: application/octet-stream" --data-binary "@/path/to/payload.json" "https://example.com/upload""#,
    )
    .unwrap();
    assert_eq!(request["body"]["mode"], "file");
    assert_eq!(request["body"]["file"][0]["filePath"], "/path/to/payload.json");
}

#[test]
fn ef_imp_01_unrecognised_commands_give_none() {
    for command in ["", "   ", "curl", "curl -H 'Accept: */*'", "curl '{{baseUrl}}/users'", "curl -H *"] {
        assert_eq!(request_from_curl(command), None, "{command:?}");
    }
}

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/curl");
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sh"))
        .collect();
    files.sort();
    files
}

fn line_diff(expected: &str, actual: &str) -> String {
    let (e, a): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
    let mut lcs = vec![vec![0usize; a.len() + 1]; e.len() + 1];
    for i in (0..e.len()).rev() {
        for j in (0..a.len()).rev() {
            lcs[i][j] = if e[i] == a[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, String::new());
    while i < e.len() || j < a.len() {
        if i < e.len() && j < a.len() && e[i] == a[j] {
            (i, j) = (i + 1, j + 1);
        } else if i < e.len() && (j == a.len() || lcs[i + 1][j] >= lcs[i][j + 1]) {
            out.push_str(&format!("  - {}\n", e[i]));
            i += 1;
        } else {
            out.push_str(&format!("  + {}\n", a[j]));
            j += 1;
        }
    }
    out
}

#[test]
fn ef_imp_01_curl_matches_bruno() {
    let files = fixtures();
    assert!(files.len() >= 60, "corpus trop petit : {}", files.len());
    let mut failures = Vec::new();
    for sh in &files {
        let name = sh.file_stem().unwrap().to_string_lossy().into_owned();
        let command = fs::read_to_string(sh).unwrap();
        let expected: Value = serde_json::from_str(&fs::read_to_string(sh.with_extension("json")).unwrap()).unwrap();
        let request_type = if name.starts_with("graphql_") { "graphql-request" } else { "http-request" };
        let actual = request_from_curl_typed(&command, request_type).unwrap_or(Value::Null);
        let (expected, actual) =
            (serde_json::to_string_pretty(&expected).unwrap(), serde_json::to_string_pretty(&actual).unwrap());
        if expected != actual {
            failures.push(format!("{name} (- Bruno, + xc) :\n{}", line_diff(&expected, &actual)));
        }
    }
    assert!(failures.is_empty(), "{} écart(s) avec Bruno :\n{}", failures.len(), failures.join("\n"));
}
