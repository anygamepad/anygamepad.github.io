//! Macros shared by the model's enums and sets.
//!
//! Every enum of the model has a stable snake_case name per variant. Names are
//! the representation in human-readable formats (TOML profiles, JSON reports,
//! the web bindings); binary formats (the embedded profile blob) store the
//! compact bit index instead, which is only stable within one library version
//! (ADR 0003: the blob is rebuilt with the library).

/// Implements `Serialize` and `Deserialize` for a type that has `as_str`,
/// `FromStr`, `bit` and `from_bit`.
macro_rules! impl_name_serde {
    ($name:ident, $kind:literal) => {
        #[cfg(feature = "serde")]
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                if serializer.is_human_readable() {
                    serializer.serialize_str(self.as_str())
                } else {
                    // Bit indices are small: below 64 for buttons, the
                    // variant index otherwise.
                    serializer.serialize_u8(self.bit() as u8)
                }
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Visitor;

                impl serde::de::Visitor<'_> for Visitor {
                    type Value = $name;

                    fn expecting(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                        write!(f, "a {} name", $kind)
                    }

                    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<$name, E> {
                        v.parse()
                            .map_err(|_| E::invalid_value(serde::de::Unexpected::Str(v), &self))
                    }

                    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<$name, E> {
                        u32::try_from(v)
                            .ok()
                            .and_then($name::from_bit)
                            .ok_or_else(|| {
                                E::invalid_value(serde::de::Unexpected::Unsigned(v), &self)
                            })
                    }
                }

                if deserializer.is_human_readable() {
                    deserializer.deserialize_str(Visitor)
                } else {
                    deserializer.deserialize_u8(Visitor)
                }
            }
        }
    };
}
pub(crate) use impl_name_serde;

/// Defines a fieldless enum whose variants have stable snake_case names.
///
/// Generates `ALL`, `COUNT`, `as_str`, `index`, the crate-internal `bit` /
/// `from_bit` used by [`enum_set!`], `Display`, `FromStr`, and with the
/// `serde` / `schemars` features a string representation.
macro_rules! named_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident: $kind:literal {
            $(
                $(#[$vmeta:meta])*
                $variant:ident = $str:literal,
            )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[non_exhaustive]
        $vis enum $name {
            $( $(#[$vmeta])* $variant, )+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: [Self; [$($str),+].len()] = [$(Self::$variant),+];

            /// Number of variants.
            pub const COUNT: usize = Self::ALL.len();

            /// Stable snake_case name, used in profiles, reports and bindings.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $str,)+
                }
            }

            /// Position in [`Self::ALL`].
            pub const fn index(self) -> usize {
                self as usize
            }

            #[allow(dead_code, reason = "only used by sets and serde")]
            pub(crate) const fn bit(self) -> u32 {
                self as u32
            }

            #[allow(dead_code, reason = "only used by sets and serde")]
            pub(crate) const fn from_bit(bit: u32) -> Option<Self> {
                if (bit as usize) < Self::COUNT {
                    Some(Self::ALL[bit as usize])
                } else {
                    None
                }
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::core::str::FromStr for $name {
            type Err = $crate::ParseNameError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($str => Ok(Self::$variant),)+
                    _ => Err($crate::ParseNameError::new($kind, s)),
                }
            }
        }

        $crate::macros::impl_name_serde!($name, $kind);

        #[cfg(feature = "schemars")]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> ::std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({
                    "type": "string",
                    "enum": [$($str),+],
                })
            }
        }
    };
}
pub(crate) use named_enum;

/// Defines a `Copy` bit set over an element type that has `bit` and
/// `from_bit` (see [`named_enum!`]).
///
/// Human-readable formats store the set as a list of names; binary formats
/// store the raw bits.
macro_rules! enum_set {
    (
        $(#[$meta:meta])*
        $vis:vis struct $set:ident($bits:ty): $elem:ident;
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
        $vis struct $set($bits);

        impl $set {
            /// The empty set.
            pub const EMPTY: Self = Self(0);

            /// Every element the set can hold.
            pub const ALL: Self = {
                let mut bits: $bits = 0;
                let mut bit = 0;
                while bit < <$bits>::BITS {
                    if $elem::from_bit(bit).is_some() {
                        bits |= 1 << bit;
                    }
                    bit += 1;
                }
                Self(bits)
            };

            /// Whether `item` is in the set.
            pub const fn contains(self, item: $elem) -> bool {
                self.0 & Self::mask(item) != 0
            }

            /// Adds `item`; returns whether it was not in the set yet.
            pub fn insert(&mut self, item: $elem) -> bool {
                let added = !self.contains(item);
                self.0 |= Self::mask(item);
                added
            }

            /// Removes `item`; returns whether it was in the set.
            pub fn remove(&mut self, item: $elem) -> bool {
                let removed = self.contains(item);
                self.0 &= !Self::mask(item);
                removed
            }

            /// Adds `item` if `present`, removes it otherwise.
            pub fn set(&mut self, item: $elem, present: bool) {
                if present {
                    self.insert(item);
                } else {
                    self.remove(item);
                }
            }

            /// This set with `item` added; usable in constants.
            pub const fn with(self, item: $elem) -> Self {
                Self(self.0 | Self::mask(item))
            }

            /// Number of elements.
            pub const fn len(self) -> usize {
                self.0.count_ones() as usize
            }

            /// Whether the set has no element.
            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }

            /// Elements in either set.
            pub const fn union(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }

            /// Elements in both sets.
            pub const fn intersection(self, other: Self) -> Self {
                Self(self.0 & other.0)
            }

            /// Elements in `self` but not in `other`.
            pub const fn difference(self, other: Self) -> Self {
                Self(self.0 & !other.0)
            }

            /// Whether every element of `self` is in `other`.
            pub const fn is_subset(self, other: Self) -> bool {
                self.0 & !other.0 == 0
            }

            /// Iterates over the elements in bit order.
            pub fn iter(self) -> impl Iterator<Item = $elem> {
                let mut bits = self.0;
                ::core::iter::from_fn(move || {
                    while bits != 0 {
                        let bit = bits.trailing_zeros();
                        bits &= bits - 1;
                        if let Some(item) = $elem::from_bit(bit) {
                            return Some(item);
                        }
                    }
                    None
                })
            }

            const fn mask(item: $elem) -> $bits {
                1 << item.bit()
            }
        }

        // Every element must fit in the set's bits.
        const _: () = {
            let mut bit = <$bits>::BITS;
            while bit <= u8::MAX as u32 {
                assert!(
                    $elem::from_bit(bit).is_none(),
                    concat!(stringify!($set), " is too small for ", stringify!($elem))
                );
                bit += 1;
            }
        };

        impl ::core::fmt::Debug for $set {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_set().entries(self.iter()).finish()
            }
        }

        impl FromIterator<$elem> for $set {
            fn from_iter<I: IntoIterator<Item = $elem>>(iter: I) -> Self {
                let mut set = Self::EMPTY;
                set.extend(iter);
                set
            }
        }

        impl Extend<$elem> for $set {
            fn extend<I: IntoIterator<Item = $elem>>(&mut self, iter: I) {
                for item in iter {
                    self.insert(item);
                }
            }
        }

        impl<const N: usize> From<[$elem; N]> for $set {
            fn from(items: [$elem; N]) -> Self {
                items.into_iter().collect()
            }
        }

        impl ::core::ops::BitOr for $set {
            type Output = Self;

            fn bitor(self, other: Self) -> Self {
                self.union(other)
            }
        }

        impl ::core::ops::BitOrAssign for $set {
            fn bitor_assign(&mut self, other: Self) {
                *self = self.union(other);
            }
        }

        impl ::core::ops::BitAnd for $set {
            type Output = Self;

            fn bitand(self, other: Self) -> Self {
                self.intersection(other)
            }
        }

        #[cfg(feature = "serde")]
        impl serde::Serialize for $set {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                if serializer.is_human_readable() {
                    serializer.collect_seq(self.iter())
                } else {
                    serde::Serialize::serialize(&self.0, serializer)
                }
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> serde::Deserialize<'de> for $set {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Visitor;

                impl<'de> serde::de::Visitor<'de> for Visitor {
                    type Value = $set;

                    fn expecting(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                        f.write_str("a list of names")
                    }

                    fn visit_seq<A: serde::de::SeqAccess<'de>>(
                        self,
                        mut seq: A,
                    ) -> Result<$set, A::Error> {
                        let mut set = $set::EMPTY;
                        while let Some(item) = seq.next_element::<$elem>()? {
                            set.insert(item);
                        }
                        Ok(set)
                    }
                }

                if deserializer.is_human_readable() {
                    deserializer.deserialize_seq(Visitor)
                } else {
                    let bits = <$bits as serde::Deserialize>::deserialize(deserializer)?;
                    if bits & !$set::ALL.0 != 0 {
                        return Err(<D::Error as serde::de::Error>::custom(concat!(
                            "unknown bits in ",
                            stringify!($set)
                        )));
                    }
                    Ok($set(bits))
                }
            }
        }

        #[cfg(feature = "schemars")]
        impl schemars::JsonSchema for $set {
            fn schema_name() -> ::std::borrow::Cow<'static, str> {
                stringify!($set).into()
            }

            fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({
                    "type": "array",
                    "items": generator.subschema_for::<$elem>(),
                    "uniqueItems": true,
                })
            }
        }
    };
}
pub(crate) use enum_set;
