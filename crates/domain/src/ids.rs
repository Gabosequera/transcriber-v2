//! Identificadores estables. Son cadenas (compatibles con los IDs V1 como
//! `clip-000001`, `layer-4c8856d5ad19`, `item-…`) envueltas en newtypes para
//! que el compilador impida mezclarlos.

use rand::RngExt;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Patrón de ID V1 para capas/items: `[a-zA-Z][a-zA-Z0-9_-]{0,79}`.
pub fn is_valid_v1_id(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    if s.len() > 80 {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 12 hex aleatorios (como `uuid4().hex[:12]` en V1).
pub fn random_hex12() -> String {
    let mut rng = rand::rng();
    let v: u64 = rng.random();
    format!("{:012x}", v & 0xffff_ffff_ffff)
}

macro_rules! string_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                $name(s.into())
            }
            pub fn random() -> Self {
                $name(format!("{}-{}", $prefix, random_hex12()))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                $name(s.to_string())
            }
        }
    };
}

string_id!(AssetId, "asset");
string_id!(SequenceId, "seq");
string_id!(TrackId, "track");
string_id!(ClipId, "clip");
string_id!(LayerId, "layer");
string_id!(ItemId, "item");
string_id!(MarkerId, "mark");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_valid() {
        let a = ClipId::random();
        let b = ClipId::random();
        assert_ne!(a, b);
        assert!(is_valid_v1_id(a.as_str()));
        assert!(is_valid_v1_id("layer-4c8856d5ad19"));
        assert!(!is_valid_v1_id("1abc"));
        assert!(!is_valid_v1_id("../foo"));
        assert!(!is_valid_v1_id(""));
    }
}
