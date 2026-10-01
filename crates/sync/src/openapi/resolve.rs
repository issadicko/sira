//! Port de `resolveRefs` (OpenAPI 3 et Swagger 2) : caches par identité d'objet et par chemin
//! de référence, objet vide provisoire pendant la résolution d'un cycle.

use std::collections::HashMap;

use serde_json::Value;

use super::js::{array_index, Heap, Js};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    OpenApi,
    Swagger,
}

pub fn resolve(heap: &Heap, spec: &Value, flavor: Flavor) -> Js {
    let mut resolver = Resolver {
        heap,
        root: spec,
        components: spec.get("components"),
        flavor,
        by_object: HashMap::new(),
        by_ref: HashMap::new(),
    };
    resolver.walk(spec)
}

struct Resolver<'a> {
    heap: &'a Heap,
    root: &'a Value,
    components: Option<&'a Value>,
    flavor: Flavor,
    by_object: HashMap<*const Value, Js>,
    by_ref: HashMap<String, Js>,
}

impl<'a> Resolver<'a> {
    fn walk(&mut self, v: &'a Value) -> Js {
        match v {
            Value::Null => Js::Null,
            Value::Bool(b) => Js::Bool(*b),
            Value::Number(n) => Js::Num(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => Js::str(s),
            Value::Array(items) => {
                let items = items.iter().map(|x| self.walk(x)).collect();
                self.heap.arr(items)
            }
            Value::Object(map) => {
                let key = v as *const Value;
                if let Some(done) = self.by_object.get(&key) {
                    return done.clone();
                }
                if let Some(reference) = self.reference(v) {
                    if let Some(done) = self.by_ref.get(reference) {
                        return done.clone();
                    }
                    let Some(target) = self.target(reference) else {
                        return self.heap.import(v);
                    };
                    let placeholder = self.heap.obj(Vec::new());
                    self.by_ref.insert(reference.to_owned(), placeholder);
                    let resolved = self.walk(target);
                    self.by_ref.insert(reference.to_owned(), resolved.clone());
                    return resolved;
                }
                let id = self.heap.reserve();
                self.by_object.insert(key, Js::Obj(id));
                let mut entries: Vec<_> = map.iter().map(|(k, x)| (k.as_str().into(), x)).collect();
                entries = super::js::js_order(entries);
                let entries = entries.into_iter().map(|(k, x)| (k, self.walk(x))).collect();
                self.heap.fill(id, entries);
                Js::Obj(id)
            }
        }
    }

    fn reference(&self, v: &'a Value) -> Option<&'a str> {
        let r = v.get("$ref")?.as_str()?;
        let valid = match self.flavor {
            Flavor::OpenApi => r.starts_with("#/components/"),
            Flavor::Swagger => !r.is_empty(),
        };
        valid.then_some(r)
    }

    fn target(&self, reference: &str) -> Option<&'a Value> {
        match self.flavor {
            Flavor::OpenApi => {
                let path = reference.replacen("#/components/", "", 1);
                path.split('/').try_fold(self.components?, |cur, key| child(cur, key).filter(|x| truthy(x)))
            }
            Flavor::Swagger => {
                let path = reference.strip_prefix("#/")?;
                path.split('/')
                    .try_fold(
                        self.root,
                        |cur, key| if cur.is_object() || cur.is_array() { child(cur, key) } else { None },
                    )
                    .filter(|x| truthy(x))
            }
        }
    }
}

fn child<'v>(v: &'v Value, key: &str) -> Option<&'v Value> {
    match v {
        Value::Object(map) => map.get(key),
        Value::Array(items) => array_index(key).and_then(|i| items.get(i)),
        _ => None,
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
