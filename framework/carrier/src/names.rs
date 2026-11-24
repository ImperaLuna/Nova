use serde::Deserialize;
use serde::de::value::{MapDeserializer, StrDeserializer};
use std::fmt;

#[derive(Debug)]
struct Variants(Option<&'static [&'static str]>);

impl fmt::Display for Variants {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "variants: {:?}", self.0)
    }
}

impl std::error::Error for Variants {}

impl serde::de::Error for Variants {
    fn custom<T: fmt::Display>(_message: T) -> Self {
        Variants(None)
    }

    fn unknown_variant(_variant: &str, expected: &'static [&'static str]) -> Self {
        Variants(Some(expected))
    }
}

const UNKNOWN: &str = "";

pub fn names_of<'de, T: Deserialize<'de>>() -> &'static [&'static str] {
    caught::<T>(T::deserialize(StrDeserializer::<Variants>::new(UNKNOWN)).err())
}

pub fn tagged_names_of<'de, T: Deserialize<'de>>(tag: &'static str) -> &'static [&'static str] {
    let message = MapDeserializer::<_, Variants>::new(std::iter::once((tag, UNKNOWN)));
    caught::<T>(T::deserialize(message).err())
}

#[expect(
    clippy::panic,
    reason = "only a type without a derived enum Deserialize gets here, which the tests of each caller catch"
)]
fn caught<T>(error: Option<Variants>) -> &'static [&'static str] {
    match error {
        Some(Variants(Some(names))) => names,
        _ => panic!(
            "serde listed no variants for {}: only a derived Deserialize of a plain or tagged enum does",
            std::any::type_name::<T>()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Plain {
        FirstOne,
        Second,
    }

    #[derive(Deserialize)]
    #[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
    #[expect(dead_code, reason = "only deserialized, for its names")]
    enum Tagged {
        Ping,
        Say { text: String },
    }

    #[test]
    fn names_follow_serde() {
        assert_eq!(names_of::<Plain>(), ["first_one", "second"]);
        assert_eq!(tagged_names_of::<Tagged>("cmd"), ["ping", "say"]);
    }
}
