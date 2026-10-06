use std::collections::HashMap;

use serde_json::{Map, Value};
use xc_core::vars::ScopeOverrides;
use xc_core::{NetworkPrefs, Overrides};

use crate::oauth2::{script_vars, SharedAuthorizer, Token};

/// Ce qui survit d'une requête à la suivante dans une collection : les variables que les scripts ont écrites.
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub runtime: Map<String, Value>,
    pub global: Map<String, Value>,
    /// Variables de l'environnement `name` réécrites par un script ; ignorées quand un autre environnement est choisi.
    pub env: Option<EnvWrites>,
    pub collection: Option<Map<String, Value>>,
    /// Les jetons OAuth 2 obtenus, par `token_key` : ils servent aux requêtes suivantes tant qu'ils sont valides.
    pub tokens: HashMap<String, Token>,
    /// Valeurs des variables secrètes de l'environnement choisi, lues dans le trousseau par l'hôte avant l'exécution.
    pub secrets: Vec<(String, String)>,
    /// Ce qui ouvre la fenêtre de connexion des flux interactifs (code d'autorisation, implicite) ; absent en CLI.
    pub authorizer: Option<SharedAuthorizer>,
    /// Les réglages réseau de l'hôte : préférences de l'application, ou options de la ligne de commande.
    pub network: NetworkPrefs,
}

#[derive(Debug, Clone, Default)]
pub struct EnvWrites {
    pub name: Option<String>,
    pub vars: Map<String, Value>,
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn pairs(map: &Map<String, Value>) -> Vec<(String, String)> {
    map.iter().map(|(k, v)| (k.clone(), text(v))).collect()
}

impl Session {
    /// Les variables runtime en texte, comme l'interpolation les lit, avec les `$oauth2.<id>.<champ>` des jetons gardés.
    pub fn runtime_strings(&self) -> HashMap<String, String> {
        let oauth2 = script_vars(&self.tokens);
        self.runtime.iter().chain(oauth2.iter()).map(|(k, v)| (k.clone(), text(v))).collect()
    }

    /// Ce que les scripts ont écrit dans l'environnement `env`, la collection et les variables globales.
    pub fn overrides(&self, env: Option<&str>) -> ScopeOverrides {
        ScopeOverrides {
            global: pairs(&self.global),
            collection: self.collection.as_ref().map(pairs),
            env: self
                .env_for(env)
                .map(|vars| pairs(vars).into_iter().filter(|(k, _)| k != xc_script::ENV_NAME).collect()),
            secrets: self.secrets.clone(),
        }
    }

    /// Ce qu'une requête de cette session ajoute à `doc` pour être préparée : les en-têtes que les scripts ont posés, les
    /// variables écrites et les réglages réseau de l'hôte.
    pub fn request_overrides(&self, headers: Option<Vec<(String, String)>>, env: Option<&str>) -> Overrides {
        Overrides { headers, vars: self.overrides(env), network: Some(self.network.clone()) }
    }

    pub fn env_for(&self, name: Option<&str>) -> Option<&Map<String, Value>> {
        self.env.as_ref().filter(|w| w.name.as_deref() == name).map(|w| &w.vars)
    }
}
