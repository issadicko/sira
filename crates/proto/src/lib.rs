//! Protobuf dynamique pour gRPC : lire des fichiers `.proto` (ou des descripteurs reçus par la réflexion du serveur),
//! lister services et méthodes, convertir un message JSON en octets protobuf et inversement. Rien ne dépend d'un code
//! généré : tout passe par les descripteurs.

mod reflection;

use std::path::{Path, PathBuf};

use prost::Message as _;
use prost_reflect::prost_types::FileDescriptorProto;
use prost_reflect::{DescriptorPool, DynamicMessage, Kind, MessageDescriptor, MethodDescriptor};
use serde::Serialize;
use serde_json::Value;

pub use reflection::{
    file_by_symbol_request, list_services_request, parse_reflection_response, ReflectionAnswer, REFLECTION_METHODS,
};

#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("fichier .proto illisible : {0}")]
    Compile(String),
    #[error("descripteur illisible : {0}")]
    Descriptor(String),
    #[error("méthode inconnue : {0}")]
    UnknownMethod(String),
    #[error("message JSON invalide pour {message} : {reason}")]
    Json { message: String, reason: String },
    #[error("message protobuf illisible ({message}) : {reason}")]
    Decode { message: String, reason: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MethodInfo {
    pub service: String,
    pub name: String,
    /// `paquet.Service/Méthode`, tel qu'écrit dans un fichier de requête et dans le chemin de l'appel.
    pub full_name: String,
    pub input: String,
    pub output: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
}

impl MethodInfo {
    /// `unary`, `server-streaming`, `client-streaming` ou `bidi-streaming` : les mots de Bruno.
    pub const fn kind(&self) -> &'static str {
        match (self.client_streaming, self.server_streaming) {
            (false, false) => "unary",
            (false, true) => "server-streaming",
            (true, false) => "client-streaming",
            (true, true) => "bidi-streaming",
        }
    }
}

fn info(method: &MethodDescriptor) -> MethodInfo {
    MethodInfo {
        service: method.parent_service().full_name().to_owned(),
        name: method.name().to_owned(),
        full_name: format!("{}/{}", method.parent_service().full_name(), method.name()),
        input: method.input().full_name().to_owned(),
        output: method.output().full_name().to_owned(),
        client_streaming: method.is_client_streaming(),
        server_streaming: method.is_server_streaming(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct Schema {
    pool: DescriptorPool,
}

impl Schema {
    /// Compile ces fichiers `.proto` ; `includes` sont les dossiers où chercher leurs imports. Le dossier de chaque
    /// fichier est ajouté d'office, comme le fait `protoc` quand on lui donne un chemin.
    pub fn from_files(files: &[PathBuf], includes: &[PathBuf]) -> Result<Self, ProtoError> {
        let mut dirs: Vec<PathBuf> = includes.to_vec();
        for file in files {
            if let Some(parent) = file.parent() {
                if !dirs.iter().any(|dir| dir == parent) {
                    dirs.push(parent.to_path_buf());
                }
            }
        }
        let mut compiler = protox::Compiler::new(dirs).map_err(|e| ProtoError::Compile(e.to_string()))?;
        compiler.include_imports(true);
        for file in files {
            compiler.open_file(file).map_err(|e| ProtoError::Compile(e.to_string()))?;
        }
        let pool = DescriptorPool::from_file_descriptor_set(compiler.file_descriptor_set())
            .map_err(|e| ProtoError::Compile(e.to_string()))?;
        Ok(Self { pool })
    }

    /// Construit le schéma de descripteurs `FileDescriptorProto` encodés (ce que la réflexion renvoie), dans
    /// n'importe quel ordre : un fichier est ajouté quand ses dépendances le sont.
    pub fn from_descriptors(encoded: &[Vec<u8>]) -> Result<Self, ProtoError> {
        let mut pending: Vec<FileDescriptorProto> = Vec::new();
        for bytes in encoded {
            let file =
                FileDescriptorProto::decode(bytes.as_slice()).map_err(|e| ProtoError::Descriptor(e.to_string()))?;
            if !pending.iter().any(|known| known.name == file.name) {
                pending.push(file);
            }
        }
        let mut pool = DescriptorPool::new();
        while !pending.is_empty() {
            let before = pending.len();
            let mut last_error = String::new();
            let mut rest = Vec::new();
            for file in pending {
                let ready = file.dependency.iter().all(|dep| pool.get_file_by_name(dep).is_some());
                if ready {
                    if let Err(error) = pool.add_file_descriptor_proto(file.clone()) {
                        last_error = error.to_string();
                        rest.push(file);
                    }
                } else {
                    rest.push(file);
                }
            }
            if rest.len() == before {
                let missing = rest
                    .iter()
                    .flat_map(|file| file.dependency.iter())
                    .find(|dep| {
                        pool.get_file_by_name(dep).is_none() && !rest.iter().any(|f| f.name.as_deref() == Some(dep))
                    })
                    .cloned();
                return Err(ProtoError::Descriptor(match missing {
                    Some(dep) => format!("dépendance manquante : {dep}"),
                    None => last_error,
                }));
            }
            pending = rest;
        }
        Ok(Self { pool })
    }

    pub fn methods(&self) -> Vec<MethodInfo> {
        self.pool.services().flat_map(|service| service.methods().map(|m| info(&m)).collect::<Vec<_>>()).collect()
    }

    fn descriptor(&self, path: &str) -> Result<MethodDescriptor, ProtoError> {
        let path = path.trim_start_matches('/');
        let (service, method) = path.split_once('/').ok_or_else(|| ProtoError::UnknownMethod(path.to_owned()))?;
        self.pool
            .get_service_by_name(service)
            .and_then(|service| service.methods().find(|m| m.name() == method))
            .ok_or_else(|| ProtoError::UnknownMethod(path.to_owned()))
    }

    pub fn method(&self, path: &str) -> Result<MethodInfo, ProtoError> {
        self.descriptor(path).map(|m| info(&m))
    }

    /// Le message JSON de la requête, converti en octets protobuf.
    pub fn encode_request(&self, path: &str, json: &str) -> Result<Vec<u8>, ProtoError> {
        let input = self.descriptor(path)?.input();
        encode(&input, json)
    }

    pub fn decode_response(&self, path: &str, bytes: &[u8]) -> Result<String, ProtoError> {
        let output = self.descriptor(path)?.output();
        decode(&output, bytes)
    }

    /// Le message d'une requête lu depuis ses octets (côté serveur, ou pour relire ce qui a été envoyé).
    pub fn decode_request(&self, path: &str, bytes: &[u8]) -> Result<String, ProtoError> {
        let input = self.descriptor(path)?.input();
        decode(&input, bytes)
    }

    /// Le message JSON d'une réponse, converti en octets protobuf (côté serveur).
    pub fn encode_response(&self, path: &str, json: &str) -> Result<Vec<u8>, ProtoError> {
        let output = self.descriptor(path)?.output();
        encode(&output, json)
    }

    /// Un message JSON vide mais complet (chaque champ à sa valeur par défaut) : le point de départ d'une requête.
    pub fn skeleton(&self, path: &str) -> Result<String, ProtoError> {
        let input = self.descriptor(path)?.input();
        Ok(serde_json::to_string_pretty(&skeleton(&input, 0)).unwrap_or_default())
    }
}

fn encode(message: &MessageDescriptor, json: &str) -> Result<Vec<u8>, ProtoError> {
    let text = if json.trim().is_empty() { "{}" } else { json };
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let dynamic = DynamicMessage::deserialize(message.clone(), &mut deserializer)
        .map_err(|e| ProtoError::Json { message: message.full_name().to_owned(), reason: e.to_string() })?;
    deserializer
        .end()
        .map_err(|e| ProtoError::Json { message: message.full_name().to_owned(), reason: e.to_string() })?;
    Ok(dynamic.encode_to_vec())
}

fn decode(message: &MessageDescriptor, bytes: &[u8]) -> Result<String, ProtoError> {
    let dynamic = DynamicMessage::decode(message.clone(), bytes)
        .map_err(|e| ProtoError::Decode { message: message.full_name().to_owned(), reason: e.to_string() })?;
    let options = prost_reflect::SerializeOptions::new().skip_default_fields(false);
    let mut out = Vec::new();
    let mut serializer =
        serde_json::Serializer::with_formatter(&mut out, serde_json::ser::PrettyFormatter::with_indent(b"  "));
    dynamic
        .serialize_with_options(&mut serializer, &options)
        .map_err(|e| ProtoError::Decode { message: message.full_name().to_owned(), reason: e.to_string() })?;
    String::from_utf8(out)
        .map_err(|e| ProtoError::Decode { message: message.full_name().to_owned(), reason: e.to_string() })
}

const DEPTH: usize = 4;

fn skeleton(message: &MessageDescriptor, depth: usize) -> Value {
    let mut map = serde_json::Map::new();
    for field in message.fields() {
        let value = if field.is_list() {
            Value::Array(Vec::new())
        } else if field.is_map() {
            Value::Object(serde_json::Map::new())
        } else {
            match field.kind() {
                Kind::Message(inner) if depth < DEPTH => skeleton(&inner, depth + 1),
                Kind::Message(_) => Value::Null,
                Kind::String => Value::String(String::new()),
                Kind::Bytes => Value::String(String::new()),
                Kind::Bool => Value::Bool(false),
                Kind::Float | Kind::Double => serde_json::json!(0.0),
                Kind::Int64 | Kind::Uint64 | Kind::Sint64 | Kind::Fixed64 | Kind::Sfixed64 => Value::String("0".into()),
                Kind::Enum(e) => Value::String(e.default_value().name().to_owned()),
                _ => serde_json::json!(0),
            }
        };
        map.insert(field.json_name().to_owned(), value);
    }
    Value::Object(map)
}

/// Où chercher un fichier de requête : le chemin du `.proto` est relatif à la racine de la collection.
pub fn resolve(root: &Path, relative: &str) -> PathBuf {
    let path = Path::new(relative);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}
