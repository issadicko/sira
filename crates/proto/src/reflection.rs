//! Messages de la réflexion gRPC (`grpc.reflection.v1.ServerReflection`), écrits à la main avec `prost` : cinq
//! messages ne justifient pas de compiler un `.proto` de plus.

use prost::Message;

/// Les deux noms que les serveurs exposent : `v1` (courant) et `v1alpha` (anciens serveurs).
pub const REFLECTION_METHODS: [&str; 2] = [
    "grpc.reflection.v1.ServerReflection/ServerReflectionInfo",
    "grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo",
];

#[derive(Clone, PartialEq, Message)]
struct Request {
    #[prost(string, tag = "1")]
    host: String,
    #[prost(oneof = "request::Kind", tags = "4, 7")]
    kind: Option<request::Kind>,
}

mod request {
    use prost::Oneof;

    #[derive(Clone, PartialEq, Oneof)]
    pub enum Kind {
        #[prost(string, tag = "4")]
        FileContainingSymbol(String),
        #[prost(string, tag = "7")]
        ListServices(String),
    }
}

#[derive(Clone, PartialEq, Message)]
struct Response {
    #[prost(oneof = "response::Kind", tags = "4, 6, 7")]
    kind: Option<response::Kind>,
}

#[derive(Clone, PartialEq, Message)]
struct FileDescriptors {
    #[prost(bytes = "vec", repeated, tag = "1")]
    file_descriptor_proto: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
struct ServiceName {
    #[prost(string, tag = "1")]
    name: String,
}

#[derive(Clone, PartialEq, Message)]
struct ListServices {
    #[prost(message, repeated, tag = "1")]
    service: Vec<ServiceName>,
}

#[derive(Clone, PartialEq, Message)]
struct ErrorResponse {
    #[prost(int32, tag = "1")]
    error_code: i32,
    #[prost(string, tag = "2")]
    error_message: String,
}

mod response {
    use prost::Oneof;

    use super::{ErrorResponse, FileDescriptors, ListServices};

    #[derive(Clone, PartialEq, Oneof)]
    pub enum Kind {
        #[prost(message, tag = "4")]
        FileDescriptors(FileDescriptors),
        #[prost(message, tag = "6")]
        ListServices(ListServices),
        #[prost(message, tag = "7")]
        Error(ErrorResponse),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReflectionAnswer {
    Services(Vec<String>),
    Files(Vec<Vec<u8>>),
    Error { code: i32, message: String },
    Other,
}

pub fn list_services_request() -> Vec<u8> {
    Request { host: String::new(), kind: Some(request::Kind::ListServices(String::new())) }.encode_to_vec()
}

pub fn file_by_symbol_request(symbol: &str) -> Vec<u8> {
    Request { host: String::new(), kind: Some(request::Kind::FileContainingSymbol(symbol.to_owned())) }.encode_to_vec()
}

pub fn parse_reflection_response(bytes: &[u8]) -> Result<ReflectionAnswer, crate::ProtoError> {
    let response =
        Response::decode(bytes).map_err(|e| crate::ProtoError::Descriptor(format!("réponse de réflexion : {e}")))?;
    Ok(match response.kind {
        Some(response::Kind::ListServices(list)) => {
            ReflectionAnswer::Services(list.service.into_iter().map(|s| s.name).collect())
        }
        Some(response::Kind::FileDescriptors(files)) => ReflectionAnswer::Files(files.file_descriptor_proto),
        Some(response::Kind::Error(error)) => {
            ReflectionAnswer::Error { code: error.error_code, message: error.error_message }
        }
        None => ReflectionAnswer::Other,
    })
}
