//! A JSON value with its objects' fields kept in the order a page's own script walks them: the keys that are array
//! indices first, by their number, then the others as written. A site whose answer is read in its own order — a
//! value read as its words, a table row given as an object, a catalogue's categories — is read as this tree, since
//! serde_json's map keeps its keys sorted.

use std::fmt;

use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Tree {
  /// A string, a number, a boolean or null.
  Scalar(Value),
  List(Vec<Tree>),
  Fields(Vec<(String, Tree)>),
}

// A key that is an array index, which a script's object walks first, by its number.
fn index_of(key: &str) -> Option<u32> {
  let shaped = key == "0"
    || (!key.starts_with('0') && !key.is_empty() && key.bytes().all(|byte| byte.is_ascii_digit()));

  shaped.then(|| key.parse::<u32>().ok()).flatten().filter(|index| *index < u32::MAX)
}

impl Tree {
  /// An object's fields as a script holds them: a field set twice keeps its first place and its last value, and the
  /// array indices stand first.
  pub fn fields(entries: impl IntoIterator<Item = (String, Tree)>) -> Tree {
    let mut fields = Tree::Fields(Vec::new());

    for (key, value) in entries {
      fields.set(key, value);
    }

    fields
  }

  /// A field set on an object as a script sets one: in its place where the object has it, and where it does not,
  /// after the others, or among the array indices by its number.
  pub fn set(&mut self, key: String, value: Tree) {
    let Tree::Fields(fields) = self else {
      return;
    };

    if let Some(known) = fields.iter_mut().find(|(name, _)| *name == key) {
      known.1 = value;
      return;
    }

    match index_of(&key) {
      Some(index) => {
        let at = fields
          .iter()
          .position(|(name, _)| index_of(name).is_none_or(|other| other > index))
          .unwrap_or(fields.len());

        fields.insert(at, (key, value));
      }
      None => fields.push((key, value)),
    }
  }

  pub fn get(&self, key: &str) -> Option<&Tree> {
    match self {
      Tree::Fields(fields) => fields.iter().find(|(name, _)| name == key).map(|(_, value)| value),
      _ => None,
    }
  }

  /// A list's items, and none of anything else.
  pub fn list(&self) -> &[Tree] {
    match self {
      Tree::List(items) => items,
      _ => &[],
    }
  }

  /// An object's fields, and none of anything else.
  pub fn entries(&self) -> &[(String, Tree)] {
    match self {
      Tree::Fields(fields) => fields,
      _ => &[],
    }
  }

  pub fn is_fields(&self) -> bool {
    matches!(self, Tree::Fields(_))
  }

  pub fn as_str(&self) -> Option<&str> {
    match self {
      Tree::Scalar(Value::String(text)) => Some(text),
      _ => None,
    }
  }

  pub fn as_f64(&self) -> Option<f64> {
    match self {
      Tree::Scalar(Value::Number(number)) => number.as_f64(),
      _ => None,
    }
  }

  pub fn is_true(&self) -> bool {
    matches!(self, Tree::Scalar(Value::Bool(true)))
  }

  /// The value as serde_json holds it, its fields' order given up.
  pub fn to_value(&self) -> Value {
    match self {
      Tree::Scalar(value) => value.clone(),
      Tree::List(items) => Value::Array(items.iter().map(Tree::to_value).collect()),
      Tree::Fields(fields) => {
        Value::Object(fields.iter().map(|(key, value)| (key.clone(), value.to_value())).collect())
      }
    }
  }
}

impl<'de> Deserialize<'de> for Tree {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    struct Walk;

    impl<'de> Visitor<'de> for Walk {
      type Value = Tree;

      fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
      }

      fn visit_bool<E>(self, value: bool) -> Result<Tree, E> {
        Ok(Tree::Scalar(Value::Bool(value)))
      }

      fn visit_i64<E>(self, value: i64) -> Result<Tree, E> {
        Ok(Tree::Scalar(value.into()))
      }

      fn visit_u64<E>(self, value: u64) -> Result<Tree, E> {
        Ok(Tree::Scalar(value.into()))
      }

      fn visit_f64<E>(self, value: f64) -> Result<Tree, E> {
        Ok(Tree::Scalar(serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)))
      }

      fn visit_str<E>(self, value: &str) -> Result<Tree, E> {
        Ok(Tree::Scalar(Value::String(value.to_owned())))
      }

      fn visit_string<E>(self, value: String) -> Result<Tree, E> {
        Ok(Tree::Scalar(Value::String(value)))
      }

      fn visit_unit<E>(self) -> Result<Tree, E> {
        Ok(Tree::Scalar(Value::Null))
      }

      fn visit_none<E>(self) -> Result<Tree, E> {
        Ok(Tree::Scalar(Value::Null))
      }

      fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Tree, A::Error> {
        let mut items = Vec::new();

        while let Some(item) = seq.next_element()? {
          items.push(item);
        }

        Ok(Tree::List(items))
      }

      fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Tree, A::Error> {
        let mut fields = Tree::Fields(Vec::new());

        while let Some((key, value)) = map.next_entry::<String, Tree>()? {
          fields.set(key, value);
        }

        Ok(fields)
      }
    }

    deserializer.deserialize_any(Walk)
  }
}
