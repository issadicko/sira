//! Port de `resolveRefs` (OpenAPI 3 et Swagger 2) : caches par identité d'objet et par chemin
//! de référence, objet vide provisoire pendant la résolution d'un cycle.

use std::collections::HashMap;

use serde_json::Value;

use super::common::MAX_DEPTH;
use super::heap::{type_error, Heap, Js};
use super::R;
use crate::js::{array_index, js_order, truthy};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    OpenApi,
    Swagger,
}

/// Résout les `$ref` de `spec` ; au-delà de [`MAX_DEPTH`] niveaux d'imbrication ou de références, la spec est refusée.
pub fn resolve(heap: &Heap, spec: &Value, flavor: Flavor) -> R<Js> {
    let mut resolver = Resolver {
        heap,
        root: spec,
        components: spec.get("components"),
        flavor,
        by_object: HashMap::new(),
        by_ref: HashMap::new(),
    };
    resolver.walk(spec, 0)
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
    fn walk(&mut self, v: &'a Value, depth: usize) -> R<Js> {
        if depth > MAX_DEPTH {
            return Err(type_error("récursion trop profonde dans les références"));
        }
        Ok(match v {
            Value::Array(items) => {
                let items = items.iter().map(|x| self.walk(x, depth + 1)).collect::<R<_>>()?;
                self.heap.arr(items)
            }
            Value::Object(map) => {
                let key = v as *const Value;
                if let Some(done) = self.by_object.get(&key) {
                    return Ok(done.clone());
                }
                if let Some(reference) = self.reference(v) {
                    if let Some(done) = self.by_ref.get(reference) {
                        return Ok(done.clone());
                    }
                    let Some(target) = self.target(reference) else {
                        return Ok(self.heap.import(v));
                    };
                    let placeholder = self.heap.obj(Vec::new());
                    self.by_ref.insert(reference.to_owned(), placeholder);
                    let resolved = self.walk(target, depth + 1)?;
                    self.by_ref.insert(reference.to_owned(), resolved.clone());
                    return Ok(resolved);
                }
                let id = self.heap.reserve();
                self.by_object.insert(key, Js::Obj(id));
                let entries: Vec<_> = map.iter().map(|(k, x)| (k.as_str().into(), x)).collect();
                let entries =
                    js_order(entries).into_iter().map(|(k, x)| Ok((k, self.walk(x, depth + 1)?))).collect::<R<_>>()?;
                self.heap.fill(id, entries);
                Js::Obj(id)
            }
            scalar => self.heap.import(scalar),
        })
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
                path.split('/').try_fold(self.components?, |cur, key| child(cur, key).filter(|x| truthy(*x)))
            }
            Flavor::Swagger => {
                let path = reference.strip_prefix("#/")?;
                path.split('/')
                    .try_fold(
                        self.root,
                        |cur, key| if cur.is_object() || cur.is_array() { child(cur, key) } else { None },
                    )
                    .filter(|x| truthy(*x))
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
