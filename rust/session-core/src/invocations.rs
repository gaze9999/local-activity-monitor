//! Static inspection of recorded exec code. The parser accepts literals only.
use serde_json::{Map, Value};

pub const INVOCATION_LIMIT: usize = 4096;
const DEPTH_LIMIT: usize = 12;
const NODE_LIMIT: usize = 20_000;

pub struct Invocation<'a> {
    pub tool: &'a str,
    pub literal: Option<Value>,
}

pub struct Nested<'a> {
    pub calls: Vec<Invocation<'a>>,
    pub truncated: bool,
}

pub fn nested_invocations(source: &str) -> Nested<'_> {
    let mut end = source.len().min(crate::LINE_LIMIT);
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = end < source.len();
    let source = &source[..end];
    let mut calls = Vec::new();
    let mut index = 0;
    while index < source.len() {
        let remaining = &source[index..];
        if let Some(width) = comment_width(remaining) {
            index += width;
        } else if matches!(remaining.as_bytes()[0], b'"' | b'\'' | b'`') {
            index += quoted_width(remaining);
        } else if let Some((tool, offset)) = tool_at(source, index) {
            if calls.len() >= INVOCATION_LIMIT {
                truncated = true;
                break;
            }
            let mut parser = Literal::new(&source[offset..]);
            let literal = match parser.value(0) {
                Some(value) => {
                    parser.trailing_trivia();
                    index = offset + parser.offset;
                    matches!(parser.rest().as_bytes().first(), Some(b')' | b',')).then_some(value)
                }
                None => {
                    index = offset;
                    None
                }
            };
            calls.push(Invocation { tool, literal });
        } else {
            index += remaining
                .chars()
                .next()
                .expect("nonempty source")
                .len_utf8();
        }
    }
    Nested { calls, truncated }
}

fn comment_width(source: &str) -> Option<usize> {
    if let Some(comment) = source.strip_prefix("//") {
        Some(2 + comment.find('\n').map_or(comment.len(), |end| end + 1))
    } else {
        let comment = source.strip_prefix("/*")?;
        Some(2 + comment.find("*/").map_or(comment.len(), |end| end + 2))
    }
}

fn tool_at(source: &str, index: usize) -> Option<(&str, usize)> {
    let tail = source[index..].strip_prefix("tools.")?;
    if source[..index].chars().next_back().is_some_and(word) {
        return None;
    }
    let first = tail.as_bytes().first().copied()?;
    if !first.is_ascii_alphabetic() && first != b'_' {
        return None;
    }
    let width = tail
        .char_indices()
        .find_map(|(index, ch)| (!word(ch)).then_some(index))?;
    if tail.as_bytes()[width] != b'(' {
        return None;
    }
    Some((&tail[..width], index + "tools.".len() + width + 1))
}

fn word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn quoted_width(source: &str) -> usize {
    let bytes = source.as_bytes();
    let quote = bytes[0];
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            byte if byte == quote => return index + 1,
            _ => index += 1,
        }
    }
    source.len()
}

struct Literal<'a> {
    source: &'a str,
    offset: usize,
    nodes: usize,
}

impl<'a> Literal<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            nodes: 0,
        }
    }

    fn rest(&self) -> &'a str {
        &self.source[self.offset..]
    }

    fn space(&mut self) {
        self.offset = self.source.len() - self.rest().trim_start().len();
    }

    fn trailing_trivia(&mut self) {
        loop {
            self.space();
            let Some(width) = comment_width(self.rest()) else {
                break;
            };
            self.offset += width;
        }
    }

    fn next(&mut self) -> Option<char> {
        let value = self.rest().chars().next()?;
        self.offset += value.len_utf8();
        Some(value)
    }

    fn consume(&mut self, token: char) -> bool {
        if self.rest().starts_with(token) {
            self.offset += token.len_utf8();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        self.nodes += 1;
        if depth > DEPTH_LIMIT || self.nodes > NODE_LIMIT {
            return None;
        }
        self.space();
        match self.rest().as_bytes().first()? {
            b'"' | b'\'' | b'`' => self.string().map(Value::String),
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            _ => self.scalar(),
        }
    }

    fn object(&mut self, depth: usize) -> Option<Value> {
        self.offset += 1;
        self.space();
        let mut items = Map::new();
        while !self.consume('}') {
            let key = if self.rest().starts_with(['\'', '"']) {
                self.string()?
            } else {
                self.key()?
            };
            self.space();
            if !self.consume(':') || items.contains_key(&key) {
                return None;
            }
            items.insert(key, self.value(depth + 1)?);
            self.space();
            if !self.consume(',') && !self.rest().starts_with('}') {
                return None;
            }
            self.space();
        }
        Some(Value::Object(items))
    }

    fn key(&mut self) -> Option<String> {
        let rest = self.rest();
        let first = rest.as_bytes().first().copied()?;
        if !first.is_ascii_alphabetic() && !matches!(first, b'_' | b'$') {
            return None;
        }
        let length = rest
            .char_indices()
            .find_map(|(index, ch)| (!word(ch) && ch != '$').then_some(index))
            .unwrap_or(rest.len());
        self.offset += length;
        Some(rest[..length].into())
    }

    fn array(&mut self, depth: usize) -> Option<Value> {
        self.offset += 1;
        self.space();
        let mut items = Vec::new();
        while !self.consume(']') {
            items.push(self.value(depth + 1)?);
            self.space();
            if !self.consume(',') && !self.rest().starts_with(']') {
                return None;
            }
            self.space();
        }
        Some(Value::Array(items))
    }

    fn scalar(&mut self) -> Option<Value> {
        for (word_text, value) in [
            ("true", Value::Bool(true)),
            ("false", Value::Bool(false)),
            ("null", Value::Null),
            ("undefined", Value::Null),
        ] {
            if self
                .rest()
                .strip_prefix(word_text)
                .is_some_and(|tail| tail.chars().next().is_none_or(|ch| !word(ch)))
            {
                self.offset += word_text.len();
                return Some(value);
            }
        }

        let start = self.offset;
        self.consume('-');
        if !self.digits() {
            return None;
        }
        let checkpoint = self.offset;
        if self.consume('.') && !self.digits() {
            self.offset = checkpoint;
        }
        let checkpoint = self.offset;
        if self.consume('e') || self.consume('E') {
            if !self.consume('+') {
                self.consume('-');
            }
            if !self.digits() {
                self.offset = checkpoint;
            }
        }
        serde_json::from_str(&self.source[start..self.offset]).ok()
    }

    fn digits(&mut self) -> bool {
        let count = self.rest().bytes().take_while(u8::is_ascii_digit).count();
        self.offset += count;
        count != 0
    }

    fn string(&mut self) -> Option<String> {
        let quote = self.next()?;
        let mut units = Vec::new();
        while let Some(ch) = self.next() {
            if ch == quote {
                let value = String::from_utf16(&units).ok()?;
                return (quote != '`' || !value.contains("${")).then_some(value);
            }
            if matches!(ch, '\r' | '\n') && quote != '`' {
                return None;
            }
            if ch != '\\' {
                units.extend_from_slice(ch.encode_utf16(&mut [0; 2]));
                continue;
            }
            match self.next()? {
                kind @ ('u' | 'x') => {
                    let width = if kind == 'u' { 4 } else { 2 };
                    let digits = self.rest().get(..width)?;
                    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        return None;
                    }
                    units.push(u16::from_str_radix(digits, 16).ok()?);
                    self.offset += width;
                }
                '\r' => {
                    self.consume('\n');
                }
                '\n' => {}
                escaped => {
                    if escaped.is_ascii_digit()
                        && (escaped != '0'
                            || self
                                .rest()
                                .as_bytes()
                                .first()
                                .is_some_and(u8::is_ascii_digit))
                    {
                        return None;
                    }
                    let value = match escaped {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        'v' => '\u{b}',
                        '0' => '\0',
                        other => other,
                    };
                    units.extend_from_slice(value.encode_utf16(&mut [0; 2]));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_comments_and_strings_and_reads_literal_objects() {
        let source = r#"
            // tools.mcp__fake__read({})
            /* tools.mcp__fake__write({}) */
            const a = 'tools.mcp__fake__read({})';
            const b = `tools.mcp__fake__read({})`;
            other_tools.mcp__fake__read({});
            await tools.mcp__demo__read({page_id: 'item-1', value: "\u8cc7\u6599\ud83d\ude00",
                flags: [true, false, undefined, null,], count: -3.2e2, code: "\x41",} /* note */);
            tools.web__run({open: [{ref_id: 'https://example.com/'}]});
        "#;
        let found = nested_invocations(source);
        assert!(!found.truncated);
        assert_eq!(
            found.calls.iter().map(|call| call.tool).collect::<Vec<_>>(),
            ["mcp__demo__read", "web__run"]
        );
        let args = found.calls[0].literal.as_ref().unwrap();
        assert_eq!(args["page_id"], "item-1");
        assert_eq!(args["value"], "資料😀");
        assert_eq!(args["flags"], serde_json::json!([true, false, null, null]));
        assert_eq!(args["count"].as_f64(), Some(-320.0));
        assert_eq!(args["code"], "A");
    }

    #[test]
    fn counts_dynamic_sites_without_assigning_literal_arguments() {
        let source = r#"
            tools.mcp__demo__read(args);
            tools.mcp__demo__read({page_id: dynamic});
            tools.mcp__demo__read({page_id: 'literal'} || args);
            tools.mcp__demo__read({page_id: `${dynamic}`});
            tools.mcp__demo__read({page_id: 'a', page_id: 'b'});
            tools.mcp__demo__read({page_id: "\ud800"});
            tools.mcp__demo__read({page_id: 'valid'});
        "#;
        let found = nested_invocations(source);
        assert_eq!(found.calls.len(), 7);
        assert!(found.calls[..6].iter().all(|call| call.literal.is_none()));
        assert_eq!(found.calls[6].literal.as_ref().unwrap()["page_id"], "valid");
    }

    #[test]
    fn limits_depth_and_invocation_count() {
        let deep = format!(
            "tools.mcp__demo__read({}0{});",
            "[".repeat(DEPTH_LIMIT + 2),
            "]".repeat(DEPTH_LIMIT + 2)
        );
        assert!(nested_invocations(&deep).calls[0].literal.is_none());
        let repeated = "tools.mcp__demo__read({});".repeat(INVOCATION_LIMIT + 1);
        let found = nested_invocations(&repeated);
        assert_eq!(found.calls.len(), INVOCATION_LIMIT);
        assert!(found.truncated);
    }
}
