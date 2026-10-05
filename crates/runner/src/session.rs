use serde_json::{Map, Value};

/// Ce qui survit d'une requête à la suivante dans une collection : les variables que les scripts ont écrites.
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub runtime: Map<String, Value>,
    pub global: Map<String, Value>,
    /// Variables de l'environnement `name` réécrites par un script ; ignorées quand un autre environnement est choisi.
    pub env: Option<EnvWrites>,
    pub collection: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Default)]
pub struct EnvWrites {
    pub name: Option<String>,
    pub vars: Map<String, Value>,
}

impl Session {
    pub fn env_for(&self, name: Option<&str>) -> Option<&Map<String, Value>> {
        self.env.as_ref().filter(|w| w.name.as_deref() == name).map(|w| &w.vars)
    }
}
