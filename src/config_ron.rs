//! Strict RON parsing for the shipped configs: a map that names a key twice is
//! an error.
//!
//! RON, like serde's own map impls, keeps the LAST value of a repeated map
//! key and says nothing, so a second `Paladin:` block in a map silently
//! replaces the first. Every config loader parses through [`from_str`]
//! instead of `ron::from_str`. It runs the ordinary typed deserialization
//! under a pass-through [`Deserializer`] that watches every map the type
//! reads, at any depth, and refuses a key it has already seen in that map.
//! Struct fields arrive the same way, so a field named twice gets the same
//! message, though serde's derive already refuses one.
//!
//! It is one mechanism for every file and every map type: a new map field in
//! any config is covered with no annotation, and nothing has to remember to
//! use a special map type. The values it produces are exactly what
//! `ron::from_str` produces, because every call reaches the same RON
//! deserializer with the same arguments and the same visitor.
//!
//! The error names the key and the path to its map (`reference_builds`,
//! `loadouts.Mage`, `talents.Foo.effects[0]`), and RON adds the line and
//! column where the repeated key was read.
//!
//! A key is compared by what it deserializes as: a string, a number, a bool,
//! a char, a unit or a unit enum variant. A map whose key is anything richer
//! (a tuple, a struct, a variant carrying data) is refused outright, so a
//! key this check cannot compare fails loudly at load instead of slipping
//! past it. Teach [`CheckedVisitor`] to render such a key before adding one.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};

/// Parse `text` as a `T`, refusing a map that names any key twice.
///
/// A drop-in for `ron::from_str` on config files: the same value on success,
/// and a [`ron::error::SpannedError`] carrying the line and column on failure.
pub fn from_str<T: DeserializeOwned>(text: &str) -> Result<T, ron::error::SpannedError> {
    let path = Path::default();
    ron::Options::default().from_str_seed(
        text,
        CheckedSeed {
            inner: PhantomData::<T>,
            path: &path,
            key: None,
        },
    )
}

/// Where in the file the parse is: map keys and struct fields, enum variants
/// and sequence indices, from the root down.
#[derive(Default)]
struct Path(RefCell<Vec<Segment>>);

enum Segment {
    Name(String),
    Index(usize),
}

impl Path {
    fn push(&self, segment: Segment) {
        self.0.borrow_mut().push(segment);
    }

    fn pop(&self) {
        self.0.borrow_mut().pop();
    }

    /// `in `a.b[2].c``, or `at the top level`.
    fn describe(&self) -> String {
        let segments = self.0.borrow();
        if segments.is_empty() {
            return "at the top level".to_string();
        }
        let mut out = String::new();
        for segment in segments.iter() {
            match segment {
                Segment::Name(name) => {
                    if !out.is_empty() {
                        out.push('.');
                    }
                    out.push_str(name);
                }
                Segment::Index(i) => out.push_str(&format!("[{i}]")),
            }
        }
        format!("in `{out}`")
    }
}

/// What a map key deserialized as, for comparing it with the map's others.
#[derive(Clone, Debug, Default, PartialEq)]
enum Captured {
    /// Nothing reached the visitor.
    #[default]
    Nothing,
    /// A key of a shape the check can compare, rendered as text.
    Text(String),
    /// A key with structure (a tuple, a struct, a variant carrying data).
    Compound,
}

fn record(key: &mut Option<&mut Captured>, captured: Captured) {
    if let Some(out) = key.as_deref_mut() {
        *out = captured;
    }
}

// ---------------------------------------------------------------------------
// The deserializer
// ---------------------------------------------------------------------------

/// Wraps a deserializer so the visitor it calls is checked. `key` is set
/// while this deserializer is reading a map key, to capture what it was.
struct CheckedDe<'p, 'c, D> {
    inner: D,
    path: &'p Path,
    key: Option<&'c mut Captured>,
}

macro_rules! forward_deserialize {
    ($($method:ident($($arg:ident: $ty:ty),*);)*) => {
        $(
            fn $method<V: Visitor<'de>>(
                self,
                $($arg: $ty,)*
                visitor: V,
            ) -> Result<V::Value, Self::Error> {
                self.inner.$method(
                    $($arg,)*
                    CheckedVisitor { inner: visitor, path: self.path, key: self.key },
                )
            }
        )*
    };
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for CheckedDe<'_, '_, D> {
    type Error = D::Error;

    forward_deserialize! {
        deserialize_any();
        deserialize_bool();
        deserialize_i8();
        deserialize_i16();
        deserialize_i32();
        deserialize_i64();
        deserialize_i128();
        deserialize_u8();
        deserialize_u16();
        deserialize_u32();
        deserialize_u64();
        deserialize_u128();
        deserialize_f32();
        deserialize_f64();
        deserialize_char();
        deserialize_str();
        deserialize_string();
        deserialize_bytes();
        deserialize_byte_buf();
        deserialize_option();
        deserialize_unit();
        deserialize_unit_struct(name: &'static str);
        deserialize_newtype_struct(name: &'static str);
        deserialize_seq();
        deserialize_tuple(len: usize);
        deserialize_tuple_struct(name: &'static str, len: usize);
        deserialize_map();
        deserialize_struct(name: &'static str, fields: &'static [&'static str]);
        deserialize_enum(name: &'static str, variants: &'static [&'static str]);
        deserialize_identifier();
        deserialize_ignored_any();
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

/// Wraps a seed so the deserializer it is handed is checked.
struct CheckedSeed<'p, 'c, S> {
    inner: S,
    path: &'p Path,
    key: Option<&'c mut Captured>,
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for CheckedSeed<'_, '_, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<S::Value, D::Error> {
        self.inner.deserialize(CheckedDe {
            inner: deserializer,
            path: self.path,
            key: self.key,
        })
    }
}

// ---------------------------------------------------------------------------
// The visitor
// ---------------------------------------------------------------------------

/// Wraps a visitor: forwards every call unchanged, records a key's value when
/// it is reading one, and checks every map, sequence and enum it is handed.
struct CheckedVisitor<'p, 'c, V> {
    inner: V,
    path: &'p Path,
    key: Option<&'c mut Captured>,
}

macro_rules! forward_scalar {
    ($($method:ident($ty:ty) => $render:expr;)*) => {
        $(
            fn $method<E: de::Error>(mut self, v: $ty) -> Result<Self::Value, E> {
                let render: fn(&$ty) -> String = $render;
                if self.key.is_some() {
                    record(&mut self.key, Captured::Text(render(&v)));
                }
                self.inner.$method(v)
            }
        )*
    };
}

impl<'de, V: Visitor<'de>> Visitor<'de> for CheckedVisitor<'_, '_, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        self.inner.expecting(formatter)
    }

    forward_scalar! {
        visit_bool(bool) => |v| v.to_string();
        visit_i8(i8) => |v| v.to_string();
        visit_i16(i16) => |v| v.to_string();
        visit_i32(i32) => |v| v.to_string();
        visit_i64(i64) => |v| v.to_string();
        visit_i128(i128) => |v| v.to_string();
        visit_u8(u8) => |v| v.to_string();
        visit_u16(u16) => |v| v.to_string();
        visit_u32(u32) => |v| v.to_string();
        visit_u64(u64) => |v| v.to_string();
        visit_u128(u128) => |v| v.to_string();
        visit_f32(f32) => |v| v.to_string();
        visit_f64(f64) => |v| v.to_string();
        visit_char(char) => |v| v.to_string();
        visit_str(&str) => |v| v.to_string();
        visit_borrowed_str(&'de str) => |v| v.to_string();
        visit_string(String) => |v| v.clone();
        visit_bytes(&[u8]) => |v| format!("{v:?}");
        visit_borrowed_bytes(&'de [u8]) => |v| format!("{v:?}");
        visit_byte_buf(Vec<u8>) => |v| format!("{v:?}");
    }

    fn visit_unit<E: de::Error>(mut self) -> Result<V::Value, E> {
        record(&mut self.key, Captured::Text("()".to_string()));
        self.inner.visit_unit()
    }

    fn visit_none<E: de::Error>(mut self) -> Result<V::Value, E> {
        record(&mut self.key, Captured::Compound);
        self.inner.visit_none()
    }

    fn visit_some<D: Deserializer<'de>>(mut self, deserializer: D) -> Result<V::Value, D::Error> {
        record(&mut self.key, Captured::Compound);
        self.inner.visit_some(CheckedDe {
            inner: deserializer,
            path: self.path,
            key: None,
        })
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        mut self,
        deserializer: D,
    ) -> Result<V::Value, D::Error> {
        record(&mut self.key, Captured::Compound);
        self.inner.visit_newtype_struct(CheckedDe {
            inner: deserializer,
            path: self.path,
            key: None,
        })
    }

    fn visit_seq<A: SeqAccess<'de>>(mut self, seq: A) -> Result<V::Value, A::Error> {
        record(&mut self.key, Captured::Compound);
        self.inner.visit_seq(CheckedSeq {
            inner: seq,
            path: self.path,
            index: 0,
        })
    }

    fn visit_map<A: MapAccess<'de>>(mut self, map: A) -> Result<V::Value, A::Error> {
        record(&mut self.key, Captured::Compound);
        self.inner.visit_map(CheckedMap {
            inner: map,
            path: self.path,
            seen: BTreeSet::new(),
            pending: None,
        })
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        self.inner.visit_enum(CheckedEnum {
            inner: data,
            path: self.path,
            key: self.key,
        })
    }
}

// ---------------------------------------------------------------------------
// Maps, sequences and enums
// ---------------------------------------------------------------------------

/// The check itself: a map that refuses a key it has already read.
struct CheckedMap<'p, A> {
    inner: A,
    path: &'p Path,
    seen: BTreeSet<String>,
    /// The key just read, named in the path while its value is read.
    pending: Option<String>,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for CheckedMap<'_, A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        let mut captured = Captured::Nothing;
        let key = self.inner.next_key_seed(CheckedSeed {
            inner: seed,
            path: self.path,
            key: Some(&mut captured),
        })?;
        if key.is_none() {
            return Ok(None);
        }
        let text = match captured {
            Captured::Text(text) => text,
            Captured::Nothing | Captured::Compound => {
                return Err(de::Error::custom(format!(
                    "a map key {} has a shape the duplicate-key check cannot compare \
                     (see src/config_ron.rs)",
                    self.path.describe()
                )));
            }
        };
        if !self.seen.insert(text.clone()) {
            return Err(de::Error::custom(format!(
                "duplicate key `{text}` {} (RON would silently keep only the last)",
                self.path.describe()
            )));
        }
        self.pending = Some(text);
        Ok(key)
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        let name = self.pending.take().unwrap_or_default();
        self.path.push(Segment::Name(name));
        let value = self.inner.next_value_seed(CheckedSeed {
            inner: seed,
            path: self.path,
            key: None,
        });
        self.path.pop();
        value
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

struct CheckedSeq<'p, A> {
    inner: A,
    path: &'p Path,
    index: usize,
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for CheckedSeq<'_, A> {
    type Error = A::Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        self.path.push(Segment::Index(self.index));
        self.index += 1;
        let element = self.inner.next_element_seed(CheckedSeed {
            inner: seed,
            path: self.path,
            key: None,
        });
        self.path.pop();
        element
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

/// An enum: its variant's name joins the path while its content is read, and
/// a unit variant read as a map key is that key's text.
struct CheckedEnum<'p, 'c, A> {
    inner: A,
    path: &'p Path,
    key: Option<&'c mut Captured>,
}

impl<'de, 'p, 'c, A: EnumAccess<'de>> EnumAccess<'de> for CheckedEnum<'p, 'c, A> {
    type Error = A::Error;
    type Variant = CheckedVariant<'p, 'c, A::Variant>;

    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Self::Variant), A::Error> {
        let mut name = Captured::Nothing;
        let (value, variant) = self.inner.variant_seed(CheckedSeed {
            inner: seed,
            path: self.path,
            key: Some(&mut name),
        })?;
        let mut key = self.key;
        record(&mut key, name.clone());
        let name = match name {
            Captured::Text(text) => text,
            _ => String::new(),
        };
        Ok((
            value,
            CheckedVariant {
                inner: variant,
                path: self.path,
                name,
                key,
            },
        ))
    }
}

struct CheckedVariant<'p, 'c, A> {
    inner: A,
    path: &'p Path,
    name: String,
    key: Option<&'c mut Captured>,
}

impl<A> CheckedVariant<'_, '_, A> {
    /// Read a variant's content with its name on the path. A variant that
    /// carries data is a key the check cannot compare.
    fn content<R>(mut self, read: impl FnOnce(A, &Path) -> R) -> R {
        record(&mut self.key, Captured::Compound);
        self.path.push(Segment::Name(self.name));
        let value = read(self.inner, self.path);
        self.path.pop();
        value
    }
}

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for CheckedVariant<'_, '_, A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        self.inner.unit_variant()
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, A::Error> {
        self.content(|inner, path| {
            inner.newtype_variant_seed(CheckedSeed {
                inner: seed,
                path,
                key: None,
            })
        })
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.content(|inner, path| {
            inner.tuple_variant(
                len,
                CheckedVisitor {
                    inner: visitor,
                    path,
                    key: None,
                },
            )
        })
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.content(|inner, path| {
            inner.struct_variant(
                fields,
                CheckedVisitor {
                    inner: visitor,
                    path,
                    key: None,
                },
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::{BTreeMap, HashMap};

    #[derive(Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
    enum Class {
        Mage,
        Priest,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    enum Effect {
        Flat(u32),
        Tagged { tag: String },
        Ranks(BTreeMap<String, u8>),
        Pair(u8, BTreeMap<String, u8>),
    }

    #[derive(Debug, Default, Deserialize, PartialEq)]
    struct Wrapped(BTreeMap<String, u8>);

    #[derive(Debug, Deserialize, PartialEq)]
    struct File {
        #[serde(default)]
        by_class: BTreeMap<Class, Vec<u8>>,
        #[serde(default)]
        by_name: HashMap<String, u32>,
        #[serde(default)]
        by_number: BTreeMap<u32, u32>,
        #[serde(default)]
        nested: BTreeMap<Class, BTreeMap<String, u8>>,
        #[serde(default)]
        effects: Vec<Effect>,
        #[serde(default)]
        first: Option<u32>,
        #[serde(default)]
        maybe: Option<BTreeMap<String, u8>>,
        #[serde(default)]
        wrapped: Wrapped,
    }

    fn err(text: &str) -> String {
        from_str::<File>(text)
            .expect_err("the duplicate is refused")
            .to_string()
    }

    #[test]
    fn a_file_without_duplicates_parses_exactly_as_ron_does() {
        let text = r#"(
            by_class: { Mage: [1, 2], Priest: [] },
            by_name: { "a": 1, "b": 2 },
            by_number: { 1: 10, 2: 20 },
            nested: { Mage: { "x": 1 }, Priest: { "x": 1 } },
            effects: [Flat(3), Tagged(tag: "t"), Ranks({ "r": 1 }), Pair(1, { "p": 1 })],
            first: Some(4),
            maybe: Some({ "m": 1 }),
            wrapped: Wrapped({ "w": 1 }),
        )"#;
        let strict: File = from_str(text).expect("parses");
        let plain: File = ron::from_str(text).expect("parses");
        assert_eq!(strict, plain);
        assert_eq!(
            strict.nested.len(),
            2,
            "a key may repeat across sibling maps"
        );
    }

    #[test]
    fn a_repeated_enum_key_is_refused_by_name_and_place() {
        let text = "(\n    by_class: {\n        Mage: [1],\n        Mage: [2],\n    },\n)";
        // Plain RON keeps the last block and says nothing: the defect.
        let plain: File = ron::from_str(text).expect("plain RON accepts it");
        assert_eq!(plain.by_class[&Class::Mage], vec![2]);
        let e = err(text);
        assert!(e.contains("duplicate key `Mage` in `by_class`"), "{e}");
        assert!(e.starts_with("4:"), "names the line of the second key: {e}");
    }

    #[test]
    fn repeated_string_and_number_keys_are_refused() {
        let e = err(r#"(by_name: { "a": 1, "b": 2, "a": 3 })"#);
        assert!(e.contains("duplicate key `a` in `by_name`"), "{e}");
        let e = err("(by_number: { 7: 1, 7: 2 })");
        assert!(e.contains("duplicate key `7` in `by_number`"), "{e}");
    }

    #[test]
    fn a_repeat_deep_in_the_file_names_its_path() {
        let e = err(r#"(nested: { Priest: { "x": 1, "x": 2 } })"#);
        assert!(e.contains("duplicate key `x` in `nested.Priest`"), "{e}");
        let e = err(r#"(effects: [Flat(1), Ranks({ "r": 1, "r": 2 })])"#);
        assert!(e.contains("duplicate key `r` in `effects[1].Ranks`"), "{e}");
        let e = err(r#"(effects: [Pair(1, { "p": 1, "p": 2 })])"#);
        assert!(
            e.contains("duplicate key `p` in `effects[0].Pair[1]`"),
            "{e}"
        );
        let e = err(r#"(maybe: Some({ "m": 1, "m": 2 }))"#);
        assert!(e.contains("duplicate key `m` in `maybe`"), "{e}");
        let e = err(r#"(wrapped: Wrapped({ "w": 1, "w": 2 }))"#);
        assert!(e.contains("duplicate key `w` in `wrapped`"), "{e}");
    }

    #[test]
    fn a_repeated_struct_field_is_refused_the_same_way() {
        let e = err("(first: Some(1), first: Some(2))");
        assert!(e.contains("duplicate key `first` at the top level"), "{e}");
        let e = err(r#"(effects: [Tagged(tag: "a", tag: "b")])"#);
        assert!(
            e.contains("duplicate key `tag` in `effects[0].Tagged`"),
            "{e}"
        );
    }

    #[test]
    fn a_key_the_check_cannot_compare_is_refused_not_passed() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Tupled {
            m: BTreeMap<(u8, u8), u8>,
        }
        let e = from_str::<Tupled>("(m: { (1, 2): 3 })")
            .expect_err("a tuple key is refused")
            .to_string();
        assert!(e.contains("cannot compare"), "{e}");
        assert!(e.contains("in `m`"), "{e}");

        // A variant carrying data is no unit key, though it starts with a
        // name: `A(1)` and `A(2)` must not compare as the same `A`.
        #[derive(Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
        #[allow(dead_code)]
        enum Keyed {
            A(u8),
        }
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct ByVariant {
            m: BTreeMap<Keyed, u8>,
        }
        let e = from_str::<ByVariant>("(m: { A(1): 1 })")
            .expect_err("a data-carrying variant key is refused")
            .to_string();
        assert!(e.contains("cannot compare"), "{e}");
    }

    #[test]
    fn other_errors_pass_through_unchanged() {
        let strict = from_str::<File>("(by_class: { Druid: [] })")
            .expect_err("an unknown variant")
            .to_string();
        let plain = ron::from_str::<File>("(by_class: { Druid: [] })")
            .expect_err("an unknown variant")
            .to_string();
        assert_eq!(strict, plain);
    }
}
