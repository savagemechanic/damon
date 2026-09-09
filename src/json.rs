//! Small bounded JSON reader/writer for Damon's fixed wire formats.
//! Objects stay as ordered rows so parsing does not depend on a map implementation.

const MAX_DEPTH: usize = 32;
const MAX_VALUES: usize = 4096;
const MAX_STRING_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(i64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, name: &str) -> Option<&Self> {
        let Self::Object(fields) = self else {
            return None;
        };
        fields
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value))
    }

    pub fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Self)]> {
        match self {
            Self::Object(fields) => Some(fields),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn fields_exact(&self, allowed: &[&str]) -> Result<(), String> {
        let fields = self.as_object().ok_or("expected JSON object")?;
        if fields
            .iter()
            .any(|(name, _)| !allowed.contains(&name.as_str()))
        {
            return Err("JSON object contains an unknown field".into());
        }
        Ok(())
    }
}

pub fn parse(input: &str) -> Result<Value, String> {
    if input.len() > MAX_STRING_BYTES {
        return Err("JSON input exceeds 64 KiB".into());
    }
    let mut parser = Parser {
        bytes: input.as_bytes(),
        position: 0,
        values: 0,
    };
    let value = parser.value(0)?;
    parser.whitespace();
    if parser.position != parser.bytes.len() {
        return Err("unexpected bytes after JSON value".into());
    }
    Ok(value)
}

pub fn quoted(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            value if value < '\u{20}' => {
                use std::fmt::Write;
                let _ = write!(output, "\\u{:04x}", value as u32);
            }
            value => output.push(value),
        }
    }
    output.push('"');
    output
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
    values: usize,
}

impl Parser<'_> {
    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return Err("JSON nesting exceeds 32 levels".into());
        }
        self.values += 1;
        if self.values > MAX_VALUES {
            return Err("JSON contains too many values".into());
        }
        self.whitespace();
        match self.peek() {
            Some(b'n') => self.word(b"null", Value::Null),
            Some(b't') => self.word(b"true", Value::Bool(true)),
            Some(b'f') => self.word(b"false", Value::Bool(false)),
            Some(b'"') => self.string().map(Value::String),
            Some(b'[') => self.array(depth + 1),
            Some(b'{') => self.object(depth + 1),
            Some(b'-' | b'0'..=b'9') => self.number().map(Value::Number),
            _ => Err("expected JSON value".into()),
        }
    }

    fn word(&mut self, expected: &[u8], value: Value) -> Result<Value, String> {
        if self
            .bytes
            .get(self.position..self.position + expected.len())
            == Some(expected)
        {
            self.position += expected.len();
            Ok(value)
        } else {
            Err("invalid JSON word".into())
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut output = String::new();
        loop {
            let byte = self.take().ok_or("unterminated JSON string")?;
            match byte {
                b'"' => return Ok(output),
                b'\\' => {
                    let escaped = self.take().ok_or("unterminated JSON escape")?;
                    match escaped {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{08}'),
                        b'f' => output.push('\u{0c}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => self.unicode_escape(&mut output)?,
                        _ => return Err("invalid JSON escape".into()),
                    }
                }
                0..=31 => return Err("control byte in JSON string".into()),
                32..=127 => output.push(char::from(byte)),
                _ => {
                    self.position -= 1;
                    let remaining = std::str::from_utf8(&self.bytes[self.position..])
                        .map_err(|_| "invalid UTF-8 in JSON string")?;
                    let character = remaining.chars().next().ok_or("invalid UTF-8")?;
                    output.push(character);
                    self.position += character.len_utf8();
                }
            }
            if output.len() > MAX_STRING_BYTES {
                return Err("JSON string exceeds 64 KiB".into());
            }
        }
    }

    fn unicode_escape(&mut self, output: &mut String) -> Result<(), String> {
        let first = self.hex16()?;
        let scalar = if (0xd800..=0xdbff).contains(&first) {
            if self.take() != Some(b'\\') || self.take() != Some(b'u') {
                return Err("missing low Unicode surrogate".into());
            }
            let second = self.hex16()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return Err("invalid low Unicode surrogate".into());
            }
            0x10000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(second) - 0xdc00)
        } else if (0xdc00..=0xdfff).contains(&first) {
            return Err("unexpected low Unicode surrogate".into());
        } else {
            u32::from(first)
        };
        output.push(char::from_u32(scalar).ok_or("invalid Unicode scalar")?);
        Ok(())
    }

    fn hex16(&mut self) -> Result<u16, String> {
        let mut value = 0_u16;
        for _ in 0..4 {
            value = value
                .checked_mul(16)
                .and_then(|current| {
                    self.take().and_then(|byte| {
                        char::from(byte)
                            .to_digit(16)
                            .and_then(|digit| current.checked_add(digit as u16))
                    })
                })
                .ok_or("invalid Unicode escape")?;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<i64, String> {
        let start = self.position;
        if self.peek() == Some(b'-') {
            self.position += 1;
        }
        match self.peek() {
            Some(b'0') => self.position += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.position += 1;
                }
            }
            _ => return Err("invalid JSON number".into()),
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err("Damon wire JSON requires integer numbers".into());
        }
        std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| String::from("invalid JSON number"))?
            .parse()
            .map_err(|_| "JSON integer is out of range".into())
    }

    fn array(&mut self, depth: usize) -> Result<Value, String> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        self.whitespace();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(Value::Array(values));
        }
        loop {
            values.push(self.value(depth)?);
            self.whitespace();
            match self.take() {
                Some(b',') => {}
                Some(b']') => return Ok(Value::Array(values)),
                _ => return Err("expected comma or closing bracket".into()),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, String> {
        self.expect(b'{')?;
        let mut fields = Vec::new();
        self.whitespace();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Value::Object(fields));
        }
        loop {
            self.whitespace();
            let name = self.string()?;
            if fields.iter().any(|(key, _)| key == &name) {
                return Err("duplicate JSON object field".into());
            }
            self.whitespace();
            self.expect(b':')?;
            let value = self.value(depth)?;
            fields.push((name, value));
            self.whitespace();
            match self.take() {
                Some(b',') => {}
                Some(b'}') => return Ok(Value::Object(fields)),
                _ => return Err("expected comma or closing brace".into()),
            }
        }
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.position += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.take() == Some(byte) {
            Ok(())
        } else {
            Err("unexpected JSON byte".into())
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn take(&mut self) -> Option<u8> {
        let value = self.peek()?;
        self.position += 1;
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nested_wire_data_and_unicode() {
        let value = parse(r#"{"name":"Damon \ud83d\ude80","rows":[1,true,null]}"#).unwrap();
        assert_eq!(value.get("name").and_then(Value::as_str), Some("Damon 🚀"));
        assert_eq!(
            value.get("rows").and_then(Value::as_array).unwrap().len(),
            3
        );
    }

    #[test]
    fn rejects_duplicate_fields_floats_and_trailing_data() {
        assert!(parse(r#"{"a":1,"a":2}"#).is_err());
        assert!(parse("1.5").is_err());
        assert!(parse("true false").is_err());
    }

    #[test]
    fn quotes_control_bytes() {
        assert_eq!(quoted("a\n\"b"), r#""a\n\"b""#);
    }
}
