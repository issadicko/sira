#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use prost::Message as _;
use tokio::net::TcpListener;
use xc_proto::Schema;

pub fn proto_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|dir| dir.join("crates/proto/tests/fixtures"))
        .find(|dir| dir.is_dir())
        .expect("fixtures de crates/proto")
}

pub fn schema() -> Schema {
    Schema::from_files(&[proto_dir().join("demo.proto")], &[proto_dir()]).unwrap()
}

#[derive(Debug, Default, Clone)]
pub struct Seen {
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub messages: Vec<String>,
}

pub type Log = Arc<Mutex<Vec<Seen>>>;

fn frame(message: &[u8]) -> Bytes {
    let mut out = BytesMut::new();
    out.extend_from_slice(&[0]);
    out.extend_from_slice(&(message.len() as u32).to_be_bytes());
    out.extend_from_slice(message);
    out.freeze()
}

fn trailers(code: u32) -> http::HeaderMap {
    let mut map = http::HeaderMap::new();
    map.insert("grpc-status", code.to_string().parse().unwrap());
    map
}

async fn next_frame(body: &mut h2::RecvStream, buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        if buffer.len() >= 5 {
            let length = u32::from_be_bytes([buffer[1], buffer[2], buffer[3], buffer[4]]) as usize;
            if buffer.len() >= 5 + length {
                let message = buffer[5..5 + length].to_vec();
                buffer.drain(..5 + length);
                return Some(message);
            }
        }
        let chunk = body.data().await?.ok()?;
        body.flow_control().release_capacity(chunk.len()).ok();
        buffer.extend_from_slice(&chunk);
    }
}

fn reflection_services() -> Vec<u8> {
    let name = b"demo.Demo";
    let mut inner = vec![0x0A, name.len() as u8];
    inner.extend_from_slice(name);
    let mut list = vec![0x0A, inner.len() as u8];
    list.extend_from_slice(&inner);
    let mut out = vec![0x32, list.len() as u8];
    out.extend_from_slice(&list);
    out
}

fn reflection_files() -> Vec<u8> {
    let compiled = protox::compile([proto_dir().join("demo.proto")], [proto_dir()]).unwrap();
    let mut files = Vec::new();
    for file in &compiled.file {
        let bytes = file.encode_to_vec();
        files.push(0x0A);
        prost::encoding::encode_varint(bytes.len() as u64, &mut files);
        files.extend_from_slice(&bytes);
    }
    let mut out = vec![0x22];
    prost::encoding::encode_varint(files.len() as u64, &mut out);
    out.extend_from_slice(&files);
    out
}

async fn serve_one(
    request: http::Request<h2::RecvStream>,
    mut respond: h2::server::SendResponse<Bytes>,
    log: Log,
    reflection: bool,
) {
    let schema = schema();
    let (parts, mut body) = request.into_parts();
    let path = parts.uri.path().trim_start_matches('/').to_owned();
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    let index = {
        let mut log = log.lock().unwrap();
        log.push(Seen { path: path.clone(), headers: headers.clone(), messages: Vec::new() });
        log.len() - 1
    };
    let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).unwrap_or_default();
    let ok = || http::Response::builder().status(200).header("content-type", "application/grpc").body(()).unwrap();
    let mut buffer = Vec::new();

    if path.starts_with("grpc.reflection.") {
        if !reflection {
            let response = http::Response::builder()
                .status(200)
                .header("content-type", "application/grpc")
                .header("grpc-status", "12")
                .header("grpc-message", "unknown service")
                .body(())
                .unwrap();
            respond.send_response(response, true).unwrap();
            return;
        }
        let mut send = respond.send_response(ok(), false).unwrap();
        while let Some(message) = next_frame(&mut body, &mut buffer).await {
            let answer = if message.first() == Some(&0x3A) { reflection_services() } else { reflection_files() };
            send.send_data(frame(&answer), false).unwrap();
        }
        send.send_trailers(trailers(0)).unwrap();
        return;
    }

    let reply = |name: &str, user_id: i64| format!(r#"{{"message":"{name}","userId":{user_id}}}"#);
    let mut send = respond.send_response(ok(), false).unwrap();
    match path.as_str() {
        "demo.Demo/Echo" => {
            let message = next_frame(&mut body, &mut buffer).await.unwrap_or_default();
            let json = schema.decode_request(&path, &message).unwrap();
            log.lock().unwrap()[index].messages.push(json.clone());
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            let text = format!(
                "salut {}|{}|{}",
                value["name"].as_str().unwrap_or(""),
                header("authorization"),
                header("x-user")
            );
            let bytes = schema.encode_response(&path, &reply(&text, value["userId"].as_i64().unwrap_or(0))).unwrap();
            send.send_data(frame(&bytes), false).unwrap();
        }
        "demo.Demo/Watch" => {
            next_frame(&mut body, &mut buffer).await;
            for n in 1..=3 {
                let bytes = schema.encode_response(&path, &reply(&format!("w{n}"), n)).unwrap();
                send.send_data(frame(&bytes), false).unwrap();
            }
        }
        "demo.Demo/Upload" => {
            let mut count = 0;
            while let Some(message) = next_frame(&mut body, &mut buffer).await {
                log.lock().unwrap()[index].messages.push(schema.decode_request(&path, &message).unwrap());
                count += 1;
            }
            let bytes = schema.encode_response(&path, &reply(&format!("{count} reçus"), count)).unwrap();
            send.send_data(frame(&bytes), false).unwrap();
        }
        "demo.Demo/Chat" => {
            while let Some(message) = next_frame(&mut body, &mut buffer).await {
                let json = schema.decode_request(&path, &message).unwrap();
                log.lock().unwrap()[index].messages.push(json.clone());
                let value: serde_json::Value = serde_json::from_str(&json).unwrap();
                let bytes = schema
                    .encode_response(&path, &reply(&format!("re:{}", value["name"].as_str().unwrap_or("")), 0))
                    .unwrap();
                send.send_data(frame(&bytes), false).unwrap();
            }
        }
        _ => {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    send.send_trailers(trailers(0)).unwrap();
}

/// Un serveur gRPC en clair sur `127.0.0.1` qui sert `demo.Demo`, avec ou sans réflexion ; renvoie son adresse.
pub async fn serve(reflection: bool) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let log: Log = Arc::default();
    let shared = log.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let log = shared.clone();
            tokio::spawn(async move {
                let Ok(mut connection) = h2::server::handshake(stream).await else { return };
                while let Some(Ok((request, respond))) = connection.accept().await {
                    tokio::spawn(serve_one(request, respond, log.clone(), reflection));
                }
            });
        }
    });
    (format!("localhost:{port}"), log)
}
