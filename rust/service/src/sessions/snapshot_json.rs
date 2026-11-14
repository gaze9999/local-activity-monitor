//! Borrow large snapshot fields while preserving the existing JSON parser boundaries.
use serde::{Deserialize, Serialize};
use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeMap;
use serde_json::{value::RawValue, Map, Value};
use std::collections::BTreeMap;
use std::io::{self, Write};

type Root<'a> = BTreeMap<String, &'a RawValue>;

pub(super) fn patch_monitor(original: &[u8], apply: impl FnOnce(&mut Map<String, Value>)) -> Option<Vec<u8>> {
    if legacy_value_required(original)? {
        let mut value: Value = serde_json::from_slice(original).ok()?;
        apply(value.get_mut("monitor")?.as_object_mut()?);
        return serde_json::to_vec(&value).ok();
    }
    serde_json::from_slice::<Checked>(original).ok()?;
    let root: Root<'_> = serde_json::from_slice(original).ok()?;
    let mut monitor: Value = serde_json::from_str(root.get("monitor")?.get()).ok()?;
    apply(monitor.as_object_mut()?);
    let value = Patched { root: &root, monitor: &monitor };
    let mut length = CountBytes(0);
    serde_json::to_writer(&mut length, &value).ok()?;
    let mut bytes = Vec::with_capacity(length.0);
    serde_json::to_writer(&mut bytes, &value).ok()?;
    Some(bytes)
}

pub(super) fn field(original: &[u8], name: &str) -> Option<Value> {
    if legacy_value_required(original)? {
        return serde_json::from_slice::<Value>(original).ok().map(|value| value[name].clone());
    }
    serde_json::from_slice::<Checked>(original).ok()?;
    let Ok(root) = serde_json::from_slice::<Root<'_>>(original) else { return Some(Value::Null); };
    root.get(name).map_or(Some(Value::Null), |value| serde_json::from_str(value.get()).ok())
}

fn legacy_value_required(original: &[u8]) -> Option<bool> {
    let text = std::str::from_utf8(original).ok()?;
    // Value interprets reserved serde keys specially. Escapes can conceal those
    // ASCII keys, so conservatively keep the original path for Unicode escapes.
    Some(text.contains("$serde_json::private::") || text.contains("\\u"))
}

struct Patched<'a> { root: &'a Root<'a>, monitor: &'a Value }
impl Serialize for Patched<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.root.len()))?;
        for (key, value) in self.root {
            if key == "monitor" { map.serialize_entry(key, self.monitor)?; }
            else { map.serialize_entry(key, value)?; }
        }
        map.end()
    }
}

struct CountBytes(usize);
impl Write for CountBytes {
    fn write(&mut self, value: &[u8]) -> io::Result<usize> {
        self.0 = self.0.checked_add(value.len()).ok_or_else(|| io::Error::other("serialized length overflow"))?;
        Ok(value.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

// RawValue alone skips Value's recursion checks. Traverse without retaining a
// tree first, using the same deserializer's normal depth and syntax checks.
struct Checked;
impl<'de> Deserialize<'de> for Checked {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CheckVisitor;
        impl<'de> Visitor<'de> for CheckVisitor {
            type Value = Checked;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result { formatter.write_str("valid JSON") }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Checked, E> { Ok(Checked) }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Checked, E> { Ok(Checked) }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Checked, E> { Ok(Checked) }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Checked, E> { Ok(Checked) }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Checked, E> { Ok(Checked) }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Checked, E> { Ok(Checked) }
            fn visit_seq<A: SeqAccess<'de>>(self, mut values: A) -> Result<Checked, A::Error> {
                while values.next_element::<Checked>()?.is_some() {} Ok(Checked)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut values: A) -> Result<Checked, A::Error> {
                while values.next_key::<IgnoredAny>()?.is_some() { values.next_value::<Checked>()?; } Ok(Checked)
            }
        }
        deserializer.deserialize_any(CheckVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn original_patch(bytes: &[u8]) -> Option<Vec<u8>> {
        let mut value: Value = serde_json::from_slice(bytes).ok()?;
        value.get_mut("monitor")?.as_object_mut()?.insert("requests".into(), json!(1));
        serde_json::to_vec(&value).ok()
    }
    #[test]
    fn snapshot_json_preserves_value_semantics_and_rejections() {
        let mut fixtures = vec![
            "null".to_string(), "[]".into(), "{}".into(), r#"{"monitor":null}"#.into(),
            r#"{"monitor":{"requests":0},"unknown":{"keep":[1,"a",null]},"codex":{"tool_statistics":[]}}"#.into(),
            r#"{"mon\u0069tor":{"requests":4},"unknown":123456789012345678901234567890}"#.into(),
            r#"{"monitor":{},"a":"\u4e2d\n\""}"#.into(),
            r#"{"monitor":{},"monitor":{"x":2},"a":1}"#.into(),
            r#"{"monitor":{},"a":{"$serde_json::private::Number":"1.2300"}}"#.into(),
            r#"{"monitor":{},"a":{"$serde_json::private::Number":"bad"}}"#.into(),
            r#"{"monitor":{},"a":{"$serde_json::private::RawValue":"[1,2]"}}"#.into(),
            r#"{"monitor":{},"a":{"$serde_json::private::Num\u0062er":"4"}}"#.into(),
            r#"{"monitor":{},"a":{"x":1,"x":2},"numeric":1.2300}"#.into(),
            r#"{"monitor":{"requests":2,"requests":3},"a":[true,false,null]}"#.into(),
            r#"{"monitor":{},"a":NaN}"#.into(), r#"{"monitor":{},"a":01}"#.into(),
            r#"{"monitor":{},"a":"\uD800"}"#.into(), r#"{"monitor":{},"a":[1,]}"#.into(),
            r#"{"monitor":{}} trailing"#.into(),
        ];
        for number in ["-0","-0.0","1.2300","1e0","1E+003","1e100000","18446744073709551616","-9223372036854775809"] {
            fixtures.push(format!("{{\"monitor\":{{}},\"numeric\":{number}}}"));
        }
        for depth in [64,126,127,128,129,256] {
            fixtures.push(format!("{{\"monitor\":{{}},\"deep\":{}0{}}}","[".repeat(depth),"]".repeat(depth)));
        }
        for fixture in fixtures {
            let expected = original_patch(fixture.as_bytes());
            let calls = std::cell::Cell::new(0);
            let actual = patch_monitor(fixture.as_bytes(), |monitor| { calls.set(calls.get()+1); monitor.insert("requests".into(), json!(1)); });
            assert_eq!(actual.is_some(), expected.is_some(), "{fixture}");
            if let (Some(expected),Some(actual)) = (expected,actual) {
                assert_eq!(calls.get(),1);
                assert_eq!(serde_json::from_slice::<Value>(&actual).unwrap(),serde_json::from_slice::<Value>(&expected).unwrap());
            } else { assert_eq!(calls.get(),0); }
        }
        assert!(patch_monitor(b"\xff", |_| panic!("invalid input must not invoke patch")).is_none());
    }

    #[test]
    fn snapshot_json_jev_extract_and_exact_capacity() {
        for bytes in [br#"{"monitor":{},"jev":{"keep":[1,2]},"other":[null,true]}"#.as_slice(),br#"{"monitor":{}}"#,br#"{"jev":{"$serde_json::private::Number":"1.50"}}"#,b"null",b"[]",b"invalid"] {
            let expected=serde_json::from_slice::<Value>(bytes).ok().map(|value|value["jev"].clone());
            assert_eq!(field(bytes,"jev"),expected);
        }
        let bytes=br#"{"monitor":{"requests":0},"codex":{"tool_statistics":[{"tool":"test","average_ms":0.0}]},"unknown":"keep"}"#;
        let actual=patch_monitor(bytes,|monitor|{monitor.insert("requests".into(),json!(100));}).unwrap();
        assert_eq!(actual.len(),actual.capacity());
        let parsed:Value=serde_json::from_slice(&actual).unwrap();assert_eq!(parsed["unknown"],"keep");assert_eq!(parsed["monitor"]["requests"],100);
    }
}
