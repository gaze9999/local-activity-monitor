use serde_json::Value;

pub(crate) fn parse(bytes: &[u8]) -> Option<Value> {
    parse_as(bytes)
}

pub(crate) fn parse_as<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    // Callers enforce raw byte limits first. Match Python's encoding prefixes,
    // but retain strict Unicode scalar validation.
    let utf32 = bytes.strip_prefix(b"\xff\xfe\x00\x00").map(|raw| (raw, false))
        .or_else(|| bytes.strip_prefix(b"\x00\x00\xfe\xff").map(|raw| (raw, true)))
        .or_else(|| match bytes {
            [0, 0, _, _, ..] => Some((bytes, true)),
            [first, 0, 0, 0, ..] if *first != 0 => Some((bytes, false)),
            _ => None,
        });
    let utf16 = bytes.strip_prefix(b"\xff\xfe").map(|raw| (raw, false))
        .or_else(|| bytes.strip_prefix(b"\xfe\xff").map(|raw| (raw, true)))
        .or_else(|| {
            let big_endian = match bytes {
                [0, second, _, _, ..] if *second != 0 => true,
                [first, 0, third, fourth, ..] if *first != 0 && (*third != 0 || *fourth != 0) => false,
                [0, _] => true,
                [_, 0] => false,
                _ => return None,
            };
            Some((bytes, big_endian))
        });
    let value = if let Some((raw, big_endian)) = utf32 {
        if raw.len() % 4 != 0 {
            None
        } else {
            raw.chunks_exact(4).map(|unit| {
                let unit = [unit[0], unit[1], unit[2], unit[3]];
                char::from_u32(if big_endian { u32::from_be_bytes(unit) } else { u32::from_le_bytes(unit) })
            }).collect::<Option<String>>()
                .and_then(|text| serde_json::from_str::<T>(&text).ok())
        }
    } else if let Some((raw, big_endian)) = utf16 {
        if raw.len() % 2 != 0 {
            None
        } else {
            let units: Vec<_> = raw.chunks_exact(2).map(|pair| {
                let pair = [pair[0], pair[1]];
                if big_endian { u16::from_be_bytes(pair) } else { u16::from_le_bytes(pair) }
            }).collect();
            String::from_utf16(&units).ok()
                .and_then(|text| serde_json::from_str::<T>(&text).ok())
        }
    } else {
        let payload = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        serde_json::from_slice::<T>(payload).ok()
    };
    value
}
