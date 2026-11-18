//! The core UUID parser accepts shorthand UUIDs. The existing wire does not.
use serde::{Deserialize, Deserializer};
use std::{str::FromStr, sync::LazyLock};
static UUID: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
    r"^([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}|00000000-0000-0000-0000-000000000000|ffffffff-ffff-ffff-ffff-ffffffffffff)$"
).expect("constant UUID regex")
});
pub fn is_wire_id(value: &str) -> bool {
    UUID.is_match(value)
}
fn parse<T: FromStr, E: serde::de::Error>(value: String) -> Result<T, E> {
    if !is_wire_id(&value) {
        return Err(E::custom("Expected an RFC UUID."));
    }
    value.parse().map_err(|_| E::custom("Expected a UUID."))
}
pub fn wire_id<'de, D: Deserializer<'de>, T: FromStr>(d: D) -> Result<T, D::Error> {
    parse(String::deserialize(d)?)
}
pub fn optional_wire_id<'de, D: Deserializer<'de>, T: FromStr>(
    d: D,
) -> Result<Option<T>, D::Error> {
    wire_id(d).map(Some)
}
pub fn wire_ids<'de, D: Deserializer<'de>, T: FromStr>(d: D) -> Result<Vec<T>, D::Error> {
    Vec::<String>::deserialize(d)?
        .into_iter()
        .map(parse)
        .collect()
}
