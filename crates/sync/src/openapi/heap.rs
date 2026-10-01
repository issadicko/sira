//! Tas de valeurs JavaScript nécessaire au port fidèle des convertisseurs de Bruno : les tableaux et objets
//! gardent leur identité, avec les conversions et lectures de propriétés du langage.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;

use super::{OpenApiError, R};
use crate::js::{array_index, js_order, json_stringify, number_to_string, number_value, string_to_number, utf16_len};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Id(usize);

/// Valeur JavaScript : les tableaux et objets vivent dans un [`Heap`] et gardent leur identité.
#[derive(Clone, Debug)]
pub enum Js {
    Undef,
    Null,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Arr(Id),
    Obj(Id),
}

pub type Entries = Vec<(Rc<str>, Js)>;

enum Slot {
    Arr(Vec<Js>),
    Obj(Entries),
}

#[derive(Default)]
pub struct Heap {
    slots: RefCell<Vec<Slot>>,
}

pub fn type_error(msg: impl Into<String>) -> OpenApiError {
    OpenApiError::Invalid(msg.into())
}

impl Js {
    pub fn str(s: &str) -> Self {
        Self::Str(s.into())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_undef(&self) -> bool {
        matches!(self, Self::Undef)
    }

    pub fn is_nullish(&self) -> bool {
        matches!(self, Self::Undef | Self::Null)
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Self::Arr(_))
    }

    pub fn truthy(&self) -> bool {
        match self {
            Self::Undef | Self::Null => false,
            Self::Bool(b) => *b,
            Self::Num(n) => *n != 0.0 && !n.is_nan(),
            Self::Str(s) => !s.is_empty(),
            Self::Arr(_) | Self::Obj(_) => true,
        }
    }

    /// `a || b`
    pub fn or(self, other: Js) -> Js {
        if self.truthy() {
            self
        } else {
            other
        }
    }

    /// `a ?? b`
    pub fn coalesce(self, other: Js) -> Js {
        if self.is_nullish() {
            other
        } else {
            self
        }
    }

    pub fn strict_eq(&self, other: &Js) -> bool {
        match (self, other) {
            (Self::Undef, Self::Undef) | (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Num(a), Self::Num(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Arr(a), Self::Arr(b)) | (Self::Obj(a), Self::Obj(b)) => a == b,
            _ => false,
        }
    }

    pub fn is_str(&self, s: &str) -> bool {
        self.as_str() == Some(s)
    }
}

impl Heap {
    fn push(&self, slot: Slot) -> Id {
        let mut slots = self.slots.borrow_mut();
        slots.push(slot);
        Id(slots.len() - 1)
    }

    pub fn arr(&self, items: Vec<Js>) -> Js {
        Js::Arr(self.push(Slot::Arr(items)))
    }

    pub fn obj(&self, entries: Entries) -> Js {
        Js::Obj(self.push(Slot::Obj(js_order(entries))))
    }

    pub fn reserve(&self) -> Id {
        self.push(Slot::Obj(Vec::new()))
    }

    pub fn fill(&self, id: Id, entries: Entries) {
        self.slots.borrow_mut()[id.0] = Slot::Obj(js_order(entries));
    }

    /// `{ ...base, k: v }` ; une valeur `undefined` retire la clé.
    pub fn spread(&self, base: &Js, overrides: &[(&str, Js)]) -> Js {
        let mut entries = self.own_entries(base);
        for (key, value) in overrides {
            let at = entries.iter().position(|(k, _)| &**k == *key);
            match (at, value.is_undef()) {
                (Some(i), true) => {
                    entries.remove(i);
                }
                (Some(i), false) => entries[i].1 = value.clone(),
                (None, false) => entries.push(((*key).into(), value.clone())),
                (None, true) => {}
            }
        }
        self.obj(entries)
    }

    fn own_entries(&self, v: &Js) -> Entries {
        match v {
            Js::Obj(id) => match &self.slots.borrow()[id.0] {
                Slot::Obj(e) => e.clone(),
                Slot::Arr(_) => Vec::new(),
            },
            Js::Arr(_) | Js::Str(_) => {
                self.values_of(v).into_iter().enumerate().map(|(i, x)| (i.to_string().into(), x)).collect()
            }
            _ => Vec::new(),
        }
    }

    fn values_of(&self, v: &Js) -> Vec<Js> {
        match v {
            Js::Arr(id) => match &self.slots.borrow()[id.0] {
                Slot::Arr(items) => items.clone(),
                Slot::Obj(_) => Vec::new(),
            },
            Js::Str(s) => s.chars().map(|c| Js::str(c.encode_utf8(&mut [0; 4]))).collect(),
            _ => Vec::new(),
        }
    }

    /// Lecture de propriété tolérante : `undefined` sur `null`/`undefined` au lieu d'une erreur.
    pub fn get(&self, v: &Js, key: &str) -> Js {
        match v {
            Js::Obj(id) => match &self.slots.borrow()[id.0] {
                Slot::Obj(entries) => entries.iter().find(|(k, _)| &**k == key).map(|(_, x)| x.clone()),
                Slot::Arr(_) => None,
            }
            .unwrap_or(Js::Undef),
            Js::Arr(id) => {
                let slots = self.slots.borrow();
                let Slot::Arr(items) = &slots[id.0] else { return Js::Undef };
                match key {
                    "length" => Js::Num(items.len() as f64),
                    _ => array_index(key).and_then(|i| items.get(i).cloned()).unwrap_or(Js::Undef),
                }
            }
            Js::Str(s) => match key {
                "length" => Js::Num(utf16_len(s) as f64),
                _ => array_index(key)
                    .and_then(|i| s.encode_utf16().nth(i))
                    .map(|unit| Js::Str(String::from_utf16_lossy(&[unit]).into()))
                    .unwrap_or(Js::Undef),
            },
            _ => Js::Undef,
        }
    }

    /// Lecture de propriété stricte : `TypeError` sur `null`/`undefined`, comme en JavaScript.
    pub fn prop(&self, v: &Js, key: &str) -> R<Js> {
        if v.is_nullish() {
            return Err(type_error(format!("lecture de « {key} » sur une valeur nulle")));
        }
        Ok(self.get(v, key))
    }

    pub fn path(&self, v: &Js, keys: &[&str]) -> Js {
        keys.iter().fold(v.clone(), |cur, k| self.get(&cur, k))
    }

    pub fn index(&self, v: &Js, i: usize) -> Js {
        self.get(v, &i.to_string())
    }

    pub fn has_own(&self, v: &Js, key: &str) -> R<bool> {
        match v {
            Js::Undef | Js::Null => Err(type_error(format!("Object.hasOwn sur une valeur nulle (« {key} »)"))),
            Js::Obj(id) => Ok(matches!(&self.slots.borrow()[id.0], Slot::Obj(e) if e.iter().any(|(k, _)| &**k == key))),
            Js::Arr(_) | Js::Str(_) => {
                Ok(key == "length" || array_index(key).is_some_and(|i| !self.index(v, i).is_undef()))
            }
            _ => Ok(false),
        }
    }

    /// `Array.isArray(v) ? v : None`
    pub fn items(&self, v: &Js) -> Option<Vec<Js>> {
        v.is_array().then(|| self.values_of(v))
    }

    /// Éléments d'un tableau sur lequel le JavaScript appelle `map`/`forEach`/`join`.
    pub fn array_strict(&self, v: &Js, what: &str) -> R<Vec<Js>> {
        self.items(v).ok_or_else(|| type_error(format!("{what} n'est pas un tableau")))
    }

    /// `Object.entries(v)`
    pub fn entries(&self, v: &Js) -> R<Entries> {
        if v.is_nullish() {
            return Err(type_error("Object.entries sur une valeur nulle"));
        }
        Ok(self.own_entries(v))
    }

    pub fn keys(&self, v: &Js) -> R<Vec<Rc<str>>> {
        Ok(self.entries(v)?.into_iter().map(|(k, _)| k).collect())
    }

    pub fn values(&self, v: &Js) -> R<Vec<Js>> {
        Ok(self.entries(v)?.into_iter().map(|(_, x)| x).collect())
    }

    /// `lodash.each` : clés numériques pour les tableaux et chaînes, rien pour les autres valeurs.
    pub fn each(&self, v: &Js) -> Vec<(Js, Js)> {
        match v {
            Js::Obj(_) => self.own_entries(v).into_iter().map(|(k, x)| (Js::Str(k), x)).collect(),
            Js::Arr(_) | Js::Str(_) => {
                self.values_of(v).into_iter().enumerate().map(|(i, x)| (Js::Num(i as f64), x)).collect()
            }
            _ => Vec::new(),
        }
    }

    /// `x > 0`
    pub fn gt_zero(&self, v: &Js) -> bool {
        self.to_number(v) > 0.0
    }

    pub fn to_number(&self, v: &Js) -> f64 {
        match v {
            Js::Undef => f64::NAN,
            Js::Null => 0.0,
            Js::Bool(b) => f64::from(u8::from(*b)),
            Js::Num(n) => *n,
            Js::Str(s) => string_to_number(s),
            Js::Arr(_) | Js::Obj(_) => string_to_number(&self.to_string(v)),
        }
    }

    /// `String(v)`
    pub fn to_string(&self, v: &Js) -> String {
        let mut out = String::new();
        self.write_string(v, &mut Vec::new(), &mut out);
        out
    }

    fn write_string(&self, v: &Js, stack: &mut Vec<Id>, out: &mut String) {
        match v {
            Js::Undef => out.push_str("undefined"),
            Js::Null => out.push_str("null"),
            Js::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Js::Num(n) => out.push_str(&number_to_string(*n)),
            Js::Str(s) => out.push_str(s),
            Js::Obj(_) => out.push_str("[object Object]"),
            Js::Arr(id) => {
                if stack.contains(id) {
                    return;
                }
                stack.push(*id);
                for (i, item) in self.values_of(v).iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if !item.is_nullish() {
                        self.write_string(item, stack, out);
                    }
                }
                stack.pop();
            }
        }
    }

    /// `array.join(sep)` avec la conversion `String` des éléments (`null`/`undefined` → vide).
    pub fn join(&self, items: &[Js], sep: &str) -> String {
        items
            .iter()
            .map(|x| if x.is_nullish() { String::new() } else { self.to_string(x) })
            .collect::<Vec<_>>()
            .join(sep)
    }

    /// `JSON.stringify(v)` ou `JSON.stringify(v, null, 2)` ; `None` pour `undefined`.
    pub fn stringify(&self, v: &Js, pretty: bool) -> R<Option<String>> {
        Ok(self.to_value(v)?.map(|value| json_stringify(&value, pretty)))
    }

    /// Valeur telle que la verrait `JSON.parse(JSON.stringify(v))` ; `None` pour `undefined`.
    pub fn to_value(&self, v: &Js) -> R<Option<Value>> {
        self.value_of(v, &mut Vec::new())
    }

    fn value_of(&self, v: &Js, stack: &mut Vec<Id>) -> R<Option<Value>> {
        Ok(Some(match v {
            Js::Undef => return Ok(None),
            Js::Null => Value::Null,
            Js::Bool(b) => Value::Bool(*b),
            Js::Num(n) => number_value(*n),
            Js::Str(s) => Value::String(s.to_string()),
            Js::Arr(id) | Js::Obj(id) => {
                if stack.contains(id) {
                    return Err(type_error("structure circulaire impossible à sérialiser en JSON"));
                }
                stack.push(*id);
                let value = if v.is_array() {
                    let mut items = Vec::new();
                    for item in self.values_of(v) {
                        items.push(self.value_of(&item, stack)?.unwrap_or(Value::Null));
                    }
                    Value::Array(items)
                } else {
                    let mut map = serde_json::Map::new();
                    for (k, item) in self.own_entries(v) {
                        if let Some(x) = self.value_of(&item, stack)? {
                            map.insert(k.to_string(), x);
                        }
                    }
                    Value::Object(map)
                };
                stack.pop();
                value
            }
        }))
    }

    /// Copie d'une valeur JSON dans le tas, clés dans l'ordre des objets JavaScript.
    pub fn import(&self, v: &Value) -> Js {
        match v {
            Value::Null => Js::Null,
            Value::Bool(b) => Js::Bool(*b),
            Value::Number(n) => Js::Num(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => Js::str(s),
            Value::Array(items) => self.arr(items.iter().map(|x| self.import(x)).collect()),
            Value::Object(map) => self.obj(map.iter().map(|(k, x)| (k.as_str().into(), self.import(x))).collect()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ef_imp_02_stringify_and_string_follow_javascript() {
        let heap = Heap::default();
        let v = heap.import(&serde_json::json!({"b": [1.0, null, "é\u{1}"], "2": {}, "a": []}));
        assert_eq!(heap.stringify(&v, false).unwrap().unwrap(), r#"{"2":{},"b":[1,null,"é\u0001"],"a":[]}"#);
        assert_eq!(
            heap.stringify(&v, true).unwrap().unwrap(),
            "{\n  \"2\": {},\n  \"b\": [\n    1,\n    null,\n    \"é\\u0001\"\n  ],\n  \"a\": []\n}"
        );
        let list = heap.get(&v, "b");
        assert_eq!(heap.to_string(&list), "1,,é\u{1}");
        assert_eq!(heap.to_string(&v), "[object Object]");
    }
}
