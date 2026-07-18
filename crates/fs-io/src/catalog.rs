//! CSV/JSON catalog ingestion with SCHEMA VALIDATION — the AISC-catalog
//! path for the frame flagship. Every cell is checked against a declared
//! column spec; violations are helpful errors naming the row, column,
//! offending text, and the expectation. Quoted CSV fields (RFC-4180
//! subset with escaped quotes) are supported.

use crate::IoError;
use std::collections::BTreeMap;

/// What a column must contain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnKind {
    /// Any nonempty string.
    Text,
    /// A finite float, optionally bounded.
    Number {
        /// Inclusive lower bound.
        min: f64,
        /// Inclusive upper bound.
        max: f64,
    },
}

/// One column's contract.
#[derive(Debug, Clone)]
pub struct ColumnSpec {
    /// Column name (must appear in the header).
    pub name: &'static str,
    /// The value contract.
    pub kind: ColumnKind,
    /// Whether empty cells are allowed.
    pub required: bool,
}

/// A catalog schema.
#[derive(Debug, Clone)]
pub struct Schema {
    /// Column contracts (order-independent; matched by header name).
    pub columns: Vec<ColumnSpec>,
}

/// A validated catalog: rows of (column name → text) with numbers
/// pre-parsed where the schema demands them.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    /// Row-major cells keyed by column name.
    pub rows: Vec<BTreeMap<String, String>>,
    /// Pre-parsed numeric views for Number columns.
    pub numbers: Vec<BTreeMap<String, f64>>,
}

/// Resource envelope for the strict catalog-JSON reader.
///
/// The limits count logical payload, not allocator metadata: input bytes include
/// JSON whitespace and delimiters, decoded bytes include every decoded key and
/// value plus every retained number lexeme, and string/number limits apply to
/// one token. All caps are checked before growing an owned payload. `BTreeMap`
/// does not expose a fallible node-reservation API, but its insertions happen
/// only after the row/member/payload caps have admitted the member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogJsonLimits {
    /// Maximum UTF-8 input bytes, including JSON syntax and whitespace.
    pub max_input_bytes: usize,
    /// Maximum objects in the top-level array.
    pub max_rows: usize,
    /// Maximum members in one object.
    pub max_members_per_object: usize,
    /// Maximum members summed over every object.
    pub max_total_members: usize,
    /// Maximum decoded UTF-8 bytes in one key or string value.
    pub max_string_bytes: usize,
    /// Maximum bytes in one retained JSON number lexeme.
    pub max_number_bytes: usize,
    /// Maximum decoded key/value/number bytes summed over the catalog.
    pub max_decoded_bytes: usize,
}

impl CatalogJsonLimits {
    /// Default world-boundary envelope for [`Schema::parse_json`].
    pub const DEFAULT: Self = Self {
        max_input_bytes: 64 * 1024 * 1024,
        max_rows: 250_000,
        max_members_per_object: 4_096,
        max_total_members: 1_000_000,
        max_string_bytes: 1024 * 1024,
        max_number_bytes: 256,
        max_decoded_bytes: 32 * 1024 * 1024,
    };
}

impl Default for CatalogJsonLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Split one CSV record (RFC-4180 subset: quoted fields, `""` escapes).
fn split_csv(line: &str, row: usize) -> Result<Vec<String>, IoError> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    cur.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                other => cur.push(other),
            }
        } else {
            match c {
                '"' if cur.is_empty() => quoted = true,
                ',' => fields.push(core::mem::take(&mut cur)),
                other => cur.push(other),
            }
        }
    }
    if quoted {
        return Err(IoError::Malformed {
            at: row,
            what: "unterminated quoted CSV field".to_string(),
        });
    }
    fields.push(cur);
    Ok(fields)
}

impl Schema {
    /// Validate one cell against its spec.
    fn check_cell(spec: &ColumnSpec, text: &str, row: usize) -> Result<Option<f64>, IoError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            if spec.required {
                return Err(IoError::Schema {
                    row,
                    column: spec.name.to_string(),
                    what: "required cell is empty".to_string(),
                });
            }
            return Ok(None);
        }
        match spec.kind {
            ColumnKind::Text => Ok(None),
            ColumnKind::Number { min, max } => {
                let v: f64 = trimmed.parse().map_err(|_| IoError::Schema {
                    row,
                    column: spec.name.to_string(),
                    what: format!("{trimmed:?} is not a number"),
                })?;
                if !v.is_finite() || v < min || v > max {
                    return Err(IoError::Schema {
                        row,
                        column: spec.name.to_string(),
                        what: format!("{v} outside the declared range [{min}, {max}]"),
                    });
                }
                Ok(Some(v))
            }
        }
    }

    /// Parse + validate a CSV catalog (first record is the header).
    ///
    /// # Errors
    /// [`IoError::Malformed`] for CSV structure; [`IoError::Schema`] with
    /// row/column/expectation for value violations; missing schema
    /// columns are named.
    pub fn parse_csv(&self, text: &str) -> Result<Catalog, IoError> {
        let mut lines = text
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty());
        let (_, header_line) = lines.next().ok_or(IoError::Malformed {
            at: 0,
            what: "empty catalog".to_string(),
        })?;
        let header = split_csv(header_line, 0)?;
        for spec in &self.columns {
            if !header.iter().any(|h| h.trim() == spec.name) {
                return Err(IoError::Schema {
                    row: 0,
                    column: spec.name.to_string(),
                    what: format!(
                        "column missing from the header (found: {})",
                        header.join(", ")
                    ),
                });
            }
        }
        let mut rows = Vec::new();
        let mut numbers = Vec::new();
        for (data_row, (ln, line)) in lines.enumerate() {
            let row_no = data_row + 1; // 1-based, header excluded
            let fields = split_csv(line, row_no)?;
            if fields.len() != header.len() {
                return Err(IoError::Malformed {
                    at: ln + 1,
                    what: format!(
                        "record has {} fields, header has {}",
                        fields.len(),
                        header.len()
                    ),
                });
            }
            let mut row = BTreeMap::new();
            let mut nums = BTreeMap::new();
            for (h, cell) in header.iter().zip(&fields) {
                row.insert(h.trim().to_string(), cell.clone());
            }
            for spec in &self.columns {
                let cell = row.get(spec.name).cloned().unwrap_or_default();
                if let Some(v) = Self::check_cell(spec, &cell, row_no)? {
                    nums.insert(spec.name.to_string(), v);
                }
            }
            rows.push(row);
            numbers.push(nums);
        }
        Ok(Catalog { rows, numbers })
    }

    /// Parse + validate a JSON catalog: an array of flat objects
    /// (string/number members). The bounded in-house reader implements strict
    /// RFC 8259 grammar and rejects anything outside that declared subset.
    ///
    /// # Errors
    /// [`IoError`] for JSON structure or schema violations.
    pub fn parse_json(&self, text: &str) -> Result<Catalog, IoError> {
        self.parse_json_with_limits(text, CatalogJsonLimits::DEFAULT)
    }

    /// Parse + validate a JSON catalog under a caller-explicit resource
    /// envelope. The accepted language is RFC 8259 JSON restricted to one
    /// top-level array of flat objects whose values are strings or numbers.
    ///
    /// # Errors
    /// [`IoError::Malformed`] identifies the first invalid byte offset;
    /// [`IoError::ResourceBound`] names the cap, limit, and refusal offset;
    /// [`IoError::Schema`] reports row/column value violations.
    pub fn parse_json_with_limits(
        &self,
        text: &str,
        limits: CatalogJsonLimits,
    ) -> Result<Catalog, IoError> {
        let rows = mini_json_array_of_objects(text, limits)?;
        let mut numbers = Vec::new();
        numbers
            .try_reserve_exact(rows.len())
            .map_err(|_| allocation_refusal("catalog numeric-row index", 0))?;
        for (i, obj) in rows.iter().enumerate() {
            let mut nums = BTreeMap::new();
            for spec in &self.columns {
                let cell = obj.get(spec.name).map(String::as_str).unwrap_or_default();
                if let Some(v) = Self::check_cell(spec, cell, i + 1)? {
                    nums.insert(spec.name.to_string(), v);
                }
            }
            numbers.push(nums);
        }
        Ok(Catalog { rows, numbers })
    }
}

fn malformed(at: usize, what: impl Into<String>) -> IoError {
    IoError::Malformed {
        at,
        what: what.into(),
    }
}

fn cap_refusal(cap: &str, limit: usize, at: usize) -> IoError {
    IoError::ResourceBound {
        what: format!("catalog JSON {cap} cap {limit} exceeded at byte offset {at}"),
    }
}

fn allocation_refusal(payload: &str, at: usize) -> IoError {
    IoError::ResourceBound {
        what: format!("allocation failed for {payload} at byte offset {at}"),
    }
}

/// Strict RFC 8259 reader for `[ {"k": "v" | number, ...}, ... ]`.
fn mini_json_array_of_objects(
    text: &str,
    limits: CatalogJsonLimits,
) -> Result<Vec<BTreeMap<String, String>>, IoError> {
    if text.len() > limits.max_input_bytes {
        return Err(cap_refusal(
            "input-byte",
            limits.max_input_bytes,
            limits.max_input_bytes,
        ));
    }
    JsonCatalogParser {
        bytes: text.as_bytes(),
        pos: 0,
        limits,
        total_members: 0,
        decoded_bytes: 0,
    }
    .parse()
}

struct JsonCatalogParser<'a> {
    bytes: &'a [u8],
    pos: usize,
    limits: CatalogJsonLimits,
    total_members: usize,
    decoded_bytes: usize,
}

impl JsonCatalogParser<'_> {
    fn parse(mut self) -> Result<Vec<BTreeMap<String, String>>, IoError> {
        self.skip_ws();
        self.expect(b'[', "expected a JSON array")?;
        self.skip_ws();

        let mut rows = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.finish_document()?;
            return Ok(rows);
        }

        loop {
            if self.peek() != Some(b'{') {
                return Err(malformed(self.pos, "expected a JSON object"));
            }
            if rows.len() >= self.limits.max_rows {
                return Err(cap_refusal("row", self.limits.max_rows, self.pos));
            }
            rows.try_reserve(1)
                .map_err(|_| allocation_refusal("catalog row index", self.pos))?;
            rows.push(self.parse_object()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_ws();
                    if self.peek() == Some(b']') {
                        return Err(malformed(self.pos, "trailing comma in JSON array"));
                    }
                }
                Some(b']') => {
                    self.pos += 1;
                    self.finish_document()?;
                    return Ok(rows);
                }
                _ => {
                    return Err(malformed(self.pos, "expected ',' or ']' after JSON object"));
                }
            }
        }
    }

    fn parse_object(&mut self) -> Result<BTreeMap<String, String>, IoError> {
        self.expect(b'{', "expected a JSON object")?;
        self.skip_ws();
        let mut object = BTreeMap::new();
        let mut object_members = 0usize;
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(object);
        }

        loop {
            let key_at = self.pos;
            if self.peek() != Some(b'"') {
                return Err(malformed(self.pos, "expected a quoted JSON object key"));
            }
            if object_members >= self.limits.max_members_per_object {
                return Err(cap_refusal(
                    "per-object member",
                    self.limits.max_members_per_object,
                    self.pos,
                ));
            }
            if self.total_members >= self.limits.max_total_members {
                return Err(cap_refusal(
                    "aggregate member",
                    self.limits.max_total_members,
                    self.pos,
                ));
            }
            let key = self.read_string()?;
            if object.contains_key(&key) {
                return Err(malformed(
                    key_at,
                    format!(
                        "duplicate JSON object key ({} decoded UTF-8 bytes)",
                        key.len()
                    ),
                ));
            }
            self.charge_decoded(key.len(), key_at)?;

            self.skip_ws();
            self.expect(b':', "expected ':' after JSON object key")?;
            self.skip_ws();
            let value_at = self.pos;
            let value = match self.peek() {
                Some(b'"') => self.read_string()?,
                Some(b'-' | b'0'..=b'9') => self.read_number()?,
                _ => {
                    return Err(malformed(
                        self.pos,
                        "expected a JSON string or number value",
                    ));
                }
            };
            self.charge_decoded(value.len(), value_at)?;

            object.insert(key, value);
            object_members += 1;
            self.total_members += 1;

            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_ws();
                    if self.peek() == Some(b'}') {
                        return Err(malformed(self.pos, "trailing comma in JSON object"));
                    }
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(object);
                }
                _ => {
                    return Err(malformed(
                        self.pos,
                        "expected ',' or '}' after JSON object member",
                    ));
                }
            }
        }
    }

    fn read_string(&mut self) -> Result<String, IoError> {
        self.expect(b'"', "expected JSON string")?;
        let mut output = String::new();
        loop {
            let chunk_start = self.pos;
            let string_remaining = self.limits.max_string_bytes.saturating_sub(output.len());
            let aggregate_prefix =
                self.decoded_bytes
                    .checked_add(output.len())
                    .ok_or_else(|| {
                        cap_refusal(
                            "aggregate decoded-byte",
                            self.limits.max_decoded_bytes,
                            self.pos,
                        )
                    })?;
            let aggregate_remaining = self
                .limits
                .max_decoded_bytes
                .saturating_sub(aggregate_prefix);
            let raw_remaining = string_remaining.min(aggregate_remaining);
            while let Some(byte) = self.peek() {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                if self.pos - chunk_start >= raw_remaining {
                    let (cap, limit) = if string_remaining <= aggregate_remaining {
                        ("string decoded-byte", self.limits.max_string_bytes)
                    } else {
                        ("aggregate decoded-byte", self.limits.max_decoded_bytes)
                    };
                    return Err(cap_refusal(cap, limit, self.pos));
                }
                self.pos += 1;
            }
            if self.pos > chunk_start {
                let chunk = core::str::from_utf8(&self.bytes[chunk_start..self.pos])
                    .map_err(|_| malformed(chunk_start, "invalid UTF-8 in JSON string"))?;
                self.append_string(&mut output, chunk, chunk_start)?;
            }

            match self.peek() {
                None => return Err(malformed(self.pos, "unterminated JSON string")),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(output);
                }
                Some(byte) if byte < 0x20 => {
                    return Err(malformed(self.pos, "raw C0 control byte in JSON string"));
                }
                Some(b'\\') => {
                    let escape_at = self.pos;
                    let escaped = *self
                        .bytes
                        .get(self.pos + 1)
                        .ok_or_else(|| malformed(escape_at, "dangling JSON string escape"))?;
                    match escaped {
                        b'"' => {
                            self.pos += 2;
                            self.append_char(&mut output, '"', escape_at)?;
                        }
                        b'\\' => {
                            self.pos += 2;
                            self.append_char(&mut output, '\\', escape_at)?;
                        }
                        b'/' => {
                            self.pos += 2;
                            self.append_char(&mut output, '/', escape_at)?;
                        }
                        b'b' => {
                            self.pos += 2;
                            self.append_char(&mut output, '\u{0008}', escape_at)?;
                        }
                        b'f' => {
                            self.pos += 2;
                            self.append_char(&mut output, '\u{000c}', escape_at)?;
                        }
                        b'n' => {
                            self.pos += 2;
                            self.append_char(&mut output, '\n', escape_at)?;
                        }
                        b'r' => {
                            self.pos += 2;
                            self.append_char(&mut output, '\r', escape_at)?;
                        }
                        b't' => {
                            self.pos += 2;
                            self.append_char(&mut output, '\t', escape_at)?;
                        }
                        b'u' => {
                            let first = self.read_hex_quad()?;
                            let scalar = if (0xd800..=0xdbff).contains(&first) {
                                let second_at = self.pos;
                                if self.bytes.get(self.pos..self.pos.saturating_add(2))
                                    != Some(&b"\\u"[..])
                                {
                                    return Err(malformed(
                                        second_at,
                                        "high surrogate must be followed by a low-surrogate escape",
                                    ));
                                }
                                let second = self.read_hex_quad()?;
                                if !(0xdc00..=0xdfff).contains(&second) {
                                    return Err(malformed(
                                        second_at,
                                        "high surrogate followed by a non-low surrogate",
                                    ));
                                }
                                0x1_0000
                                    + (((u32::from(first) - 0xd800) << 10)
                                        | (u32::from(second) - 0xdc00))
                            } else if (0xdc00..=0xdfff).contains(&first) {
                                return Err(malformed(
                                    escape_at,
                                    "unpaired low surrogate in JSON string",
                                ));
                            } else {
                                u32::from(first)
                            };
                            let character = char::from_u32(scalar).ok_or_else(|| {
                                malformed(escape_at, "invalid Unicode scalar in JSON string")
                            })?;
                            self.append_char(&mut output, character, escape_at)?;
                        }
                        _ => {
                            return Err(malformed(
                                self.pos + 1,
                                format!("unknown JSON string escape byte 0x{escaped:02x}"),
                            ));
                        }
                    }
                }
                Some(_) => {
                    return Err(malformed(
                        self.pos,
                        "invalid byte reached the JSON string decoder",
                    ));
                }
            }
        }
    }

    fn read_hex_quad(&mut self) -> Result<u16, IoError> {
        if self.peek() != Some(b'\\') || self.bytes.get(self.pos + 1) != Some(&b'u') {
            return Err(malformed(self.pos, "expected a Unicode escape"));
        }
        self.pos += 2;
        let mut value = 0u16;
        for _ in 0..4 {
            let at = self.pos;
            let byte = *self
                .bytes
                .get(self.pos)
                .ok_or_else(|| malformed(at, "truncated four-digit Unicode escape"))?;
            let digit = match byte {
                b'0'..=b'9' => u16::from(byte - b'0'),
                b'a'..=b'f' => u16::from(byte - b'a' + 10),
                b'A'..=b'F' => u16::from(byte - b'A' + 10),
                _ => return Err(malformed(at, "non-hex digit in Unicode escape")),
            };
            value = (value << 4) | digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn read_number(&mut self) -> Result<String, IoError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.advance_number_byte(start)?;
        }
        match self.peek() {
            Some(b'0') => {
                self.advance_number_byte(start)?;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(malformed(
                        self.pos,
                        "leading zero in JSON number integer part",
                    ));
                }
            }
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.advance_number_byte(start)?;
                }
            }
            _ => {
                return Err(malformed(
                    self.pos,
                    "expected a digit in JSON number integer part",
                ));
            }
        }
        if self.peek() == Some(b'.') {
            self.advance_number_byte(start)?;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(malformed(
                    self.pos,
                    "expected a digit after JSON number decimal point",
                ));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.advance_number_byte(start)?;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.advance_number_byte(start)?;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.advance_number_byte(start)?;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(malformed(
                    self.pos,
                    "expected a digit in JSON number exponent",
                ));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.advance_number_byte(start)?;
            }
        }
        if let Some(byte) = self.peek()
            && !matches!(byte, b' ' | b'\n' | b'\r' | b'\t' | b',' | b'}')
        {
            return Err(malformed(self.pos, "invalid byte after JSON number"));
        }

        let token = &self.bytes[start..self.pos];
        if self
            .decoded_bytes
            .checked_add(token.len())
            .is_none_or(|total| total > self.limits.max_decoded_bytes)
        {
            return Err(cap_refusal(
                "aggregate decoded-byte",
                self.limits.max_decoded_bytes,
                start,
            ));
        }
        let mut output = String::new();
        output
            .try_reserve_exact(token.len())
            .map_err(|_| allocation_refusal("JSON number token", start))?;
        let token = core::str::from_utf8(token)
            .map_err(|_| malformed(start, "non-ASCII byte in JSON number"))?;
        output.push_str(token);
        Ok(output)
    }

    fn advance_number_byte(&mut self, start: usize) -> Result<(), IoError> {
        if self.pos - start >= self.limits.max_number_bytes {
            return Err(cap_refusal(
                "number-token byte",
                self.limits.max_number_bytes,
                self.pos,
            ));
        }
        self.pos += 1;
        Ok(())
    }

    fn append_char(&self, output: &mut String, character: char, at: usize) -> Result<(), IoError> {
        let mut encoded = [0u8; 4];
        self.append_string(output, character.encode_utf8(&mut encoded), at)
    }

    fn append_string(&self, output: &mut String, text: &str, at: usize) -> Result<(), IoError> {
        let new_len = output
            .len()
            .checked_add(text.len())
            .ok_or_else(|| cap_refusal("string decoded-byte", self.limits.max_string_bytes, at))?;
        if new_len > self.limits.max_string_bytes {
            return Err(cap_refusal(
                "string decoded-byte",
                self.limits.max_string_bytes,
                at,
            ));
        }
        if self
            .decoded_bytes
            .checked_add(new_len)
            .is_none_or(|total| total > self.limits.max_decoded_bytes)
        {
            return Err(cap_refusal(
                "aggregate decoded-byte",
                self.limits.max_decoded_bytes,
                at,
            ));
        }
        output
            .try_reserve(text.len())
            .map_err(|_| allocation_refusal("decoded JSON string", at))?;
        output.push_str(text);
        Ok(())
    }

    fn charge_decoded(&mut self, amount: usize, at: usize) -> Result<(), IoError> {
        self.decoded_bytes = self
            .decoded_bytes
            .checked_add(amount)
            .filter(|total| *total <= self.limits.max_decoded_bytes)
            .ok_or_else(|| {
                cap_refusal("aggregate decoded-byte", self.limits.max_decoded_bytes, at)
            })?;
        Ok(())
    }

    fn finish_document(&mut self) -> Result<(), IoError> {
        self.skip_ws();
        if self.pos == self.bytes.len() {
            Ok(())
        } else {
            Err(malformed(self.pos, "trailing bytes after the JSON array"))
        }
    }

    fn expect(&mut self, expected: u8, what: &str) -> Result<(), IoError> {
        if self.peek() == Some(expected) {
            self.pos += 1;
            Ok(())
        } else {
            Err(malformed(self.pos, what))
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_rows(input: &str) -> Result<Vec<BTreeMap<String, String>>, IoError> {
        mini_json_array_of_objects(input, CatalogJsonLimits::DEFAULT)
    }

    fn assert_malformed(input: &str, expected_at: usize, expected_detail: &str, case: &str) {
        match parse_rows(input) {
            Err(IoError::Malformed { at, what }) => {
                assert_eq!(at, expected_at, "{case}: wrong refusal offset: {what}");
                assert!(
                    what.contains(expected_detail),
                    "{case}: expected detail {expected_detail:?}, got {what:?} at {at}"
                );
            }
            other => panic!("{case}: expected Malformed, got {other:?}"),
        }
    }

    fn assert_resource(input: &str, limits: CatalogJsonLimits, expected_cap: &str, case: &str) {
        match mini_json_array_of_objects(input, limits) {
            Err(IoError::ResourceBound { what }) => assert!(
                what.contains(expected_cap) && what.contains("byte offset"),
                "{case}: expected cap {expected_cap:?} with offset, got {what:?}"
            ),
            other => panic!("{case}: expected ResourceBound, got {other:?}"),
        }
    }

    /// G0: every RFC 8259 string escape, BMP escape, surrogate pair, and raw
    /// UTF-8 scalar decodes exactly once into the retained catalog payload.
    #[test]
    fn g0_json_string_escapes_and_surrogates_decode_exactly() {
        let input = r#"[{"simple":"\"\\\/\b\f\n\r\t","nul":"\u0000","bmp":"\u20aC","pair":"\uD834\uDd1E","first":"\uD800\uDC00","last":"\uDBFF\uDFFF","raw":"café–90"}]"#;
        let rows = parse_rows(input).expect("complete RFC string fixture must parse");
        assert_eq!(rows.len(), 1, "one input object must remain one row");
        assert_eq!(
            rows[0]["simple"], "\"\\/\u{0008}\u{000c}\n\r\t",
            "all eight simple escapes must decode with their RFC meaning"
        );
        assert_eq!(rows[0]["nul"], "\0", "escaped NUL is legal JSON text");
        assert_eq!(rows[0]["bmp"], "€", "mixed-case hex digits are legal");
        assert_eq!(
            rows[0]["pair"], "𝄞",
            "UTF-16 surrogate pair must become one scalar"
        );
        assert_eq!(
            rows[0]["first"], "\u{10000}",
            "lowest surrogate pair must map to the first supplementary scalar"
        );
        assert_eq!(
            rows[0]["last"], "\u{10ffff}",
            "highest surrogate pair must map to the last Unicode scalar"
        );
        assert_eq!(rows[0]["raw"], "café–90", "raw UTF-8 must remain exact");
    }

    /// G0: the first malformed escape byte is stable and actionable.
    #[test]
    fn g0_json_malformed_unicode_and_escape_offsets_are_exact() {
        let bad_hex = r#"[{"k":"\u12G4"}]"#;
        assert_malformed(
            bad_hex,
            bad_hex.find('G').expect("fixture has G"),
            "non-hex",
            "bad Unicode hex digit",
        );

        let lone_low = r#"[{"k":"\uDC00"}]"#;
        assert_malformed(
            lone_low,
            lone_low.find("\\uDC00").expect("fixture has low surrogate"),
            "unpaired low surrogate",
            "lone low surrogate",
        );

        let lone_high = r#"[{"k":"\uD800"}]"#;
        assert_malformed(
            lone_high,
            lone_high
                .find("\\uD800")
                .expect("fixture has high surrogate")
                + 6,
            "high surrogate",
            "lone high surrogate",
        );

        let wrong_pair = r#"[{"k":"\uD800\u0041"}]"#;
        assert_malformed(
            wrong_pair,
            wrong_pair
                .find("\\u0041")
                .expect("fixture has second escape"),
            "non-low surrogate",
            "high surrogate followed by BMP scalar",
        );

        let truncated = r#"[{"k":"\u12"#;
        assert_malformed(
            truncated,
            truncated.len(),
            "truncated",
            "truncated Unicode escape",
        );

        let dangling = "[{\"k\":\"\\";
        assert_malformed(
            dangling,
            dangling.len() - 1,
            "dangling",
            "dangling terminal escape",
        );

        let prefix = b"[{\"k\":\"";
        let suffix = b"\"}]";
        for escaped in 0u8..=0x7f {
            if matches!(
                escaped,
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' | b'u'
            ) {
                continue;
            }
            let mut bytes = Vec::from(prefix);
            bytes.push(b'\\');
            bytes.push(escaped);
            bytes.extend_from_slice(suffix);
            let input = String::from_utf8(bytes).expect("ASCII escape fixture is UTF-8");
            assert_malformed(
                &input,
                prefix.len() + 1,
                "unknown JSON string escape",
                &format!("unknown escape byte 0x{escaped:02x}"),
            );
        }

        let incomplete_u = r#"[{"k":"\u"}]"#;
        assert_malformed(
            incomplete_u,
            incomplete_u.find("\\u").expect("fixture has escape") + 2,
            "non-hex",
            "Unicode escape without digits",
        );
    }

    /// G0: all 32 raw C0 bytes are forbidden inside a JSON string, including
    /// the four bytes that are legal only as whitespace outside strings.
    #[test]
    fn g0_json_raw_c0_controls_are_exhaustively_rejected() {
        let prefix = b"[{\"k\":\"";
        let suffix = b"\"}]";
        for control in 0u8..=0x1f {
            let mut bytes = Vec::from(prefix);
            bytes.push(control);
            bytes.extend_from_slice(suffix);
            let input = String::from_utf8(bytes).expect("C0 fixture remains valid UTF-8");
            assert_malformed(
                &input,
                prefix.len(),
                "raw C0 control",
                &format!("raw control byte 0x{control:02x}"),
            );
        }
    }

    /// G0: JSON's number production is implemented literally and preserves
    /// each admitted lexeme for downstream schema validation/provenance.
    #[test]
    fn g0_json_number_grammar_accepts_only_rfc_8259_lexemes() {
        for number in [
            "0",
            "-0",
            "10",
            "-10",
            "0.0",
            "-0.125",
            "1e2",
            "1E+2",
            "1e-2",
            "1234567890.0123456789e+123",
        ] {
            let input = format!("[{{\"n\":{number}}}]");
            let rows = parse_rows(&input)
                .unwrap_or_else(|error| panic!("valid number {number:?} refused: {error:?}"));
            assert_eq!(
                rows[0]["n"], number,
                "valid number {number:?} must retain its exact lexeme"
            );
        }

        let prefix = "[{\"n\":";
        for (number, relative_at, detail) in [
            ("+1", 0, "expected a JSON string or number"),
            ("01", 1, "leading zero"),
            ("-01", 2, "leading zero"),
            (".1", 0, "expected a JSON string or number"),
            ("1.", 2, "after JSON number decimal point"),
            ("1e", 2, "JSON number exponent"),
            ("1e+", 3, "JSON number exponent"),
            ("--1", 1, "integer part"),
            ("1_0", 1, "invalid byte after JSON number"),
            ("0x1", 1, "invalid byte after JSON number"),
            ("NaN", 0, "expected a JSON string or number"),
            ("Infinity", 0, "expected a JSON string or number"),
            ("1e--2", 3, "JSON number exponent"),
            ("1..0", 2, "after JSON number decimal point"),
        ] {
            let input = format!("{prefix}{number}}}]");
            assert_malformed(
                &input,
                prefix.len() + relative_at,
                detail,
                &format!("invalid number {number:?}"),
            );
        }
    }

    /// G0: comma/colon grammar is explicit, and duplicate detection operates
    /// on decoded keys so alternate escape spellings cannot overwrite data.
    #[test]
    fn g0_json_delimiters_and_duplicate_keys_are_strict() {
        let missing_object_comma = r#"[{"a":"1" "b":"2"}]"#;
        assert_malformed(
            missing_object_comma,
            missing_object_comma.rfind("\"b\"").expect("second key"),
            "expected ',' or '}'",
            "missing object comma",
        );

        let trailing_object_comma = r#"[{"a":"1",}]"#;
        assert_malformed(
            trailing_object_comma,
            trailing_object_comma.find('}').expect("object close"),
            "trailing comma in JSON object",
            "trailing object comma",
        );

        let missing_array_comma = r#"[{"a":"1"} {"b":"2"}]"#;
        assert_malformed(
            missing_array_comma,
            missing_array_comma.rfind('{').expect("second object"),
            "expected ',' or ']'",
            "missing array comma",
        );

        let trailing_array_comma = r#"[{"a":"1"},]"#;
        assert_malformed(
            trailing_array_comma,
            trailing_array_comma.find(']').expect("array close"),
            "trailing comma in JSON array",
            "trailing array comma",
        );

        let doubled_comma = r#"[{"a":"1",,"b":"2"}]"#;
        assert_malformed(
            doubled_comma,
            doubled_comma.find(",,").expect("double comma") + 1,
            "quoted JSON object key",
            "doubled object comma",
        );

        let missing_colon = r#"[{"a" "1"}]"#;
        assert_malformed(
            missing_colon,
            missing_colon.rfind("\"1\"").expect("value"),
            "expected ':'",
            "missing colon",
        );

        let duplicate = r#"[{"a":"1","a":"2"}]"#;
        assert_malformed(
            duplicate,
            duplicate.rfind("\"a\"").expect("second key"),
            "duplicate JSON object key",
            "literal duplicate key",
        );

        let decoded_duplicate = r#"[{"a":"1","\u0061":"2"}]"#;
        assert_malformed(
            decoded_duplicate,
            decoded_duplicate.find("\"\\u0061\"").expect("escaped key"),
            "duplicate JSON object key",
            "escape-equivalent duplicate key",
        );

        for (input, case) in [
            (r#"[{"a":true}]"#, "boolean value"),
            (r#"[{"a":null}]"#, "null value"),
            (r#"[{"a":[]}]"#, "nested array value"),
            (r#"[{"a":{}}]"#, "nested object value"),
            (r#"[1]"#, "non-object array member"),
        ] {
            assert!(
                matches!(parse_rows(input), Err(IoError::Malformed { .. })),
                "{case}: unsupported flat-catalog value must be Malformed"
            );
        }
    }

    /// G3: insignificant whitespace, key ordering, raw Unicode, and escaped
    /// Unicode are semantic-preserving rewrites of this restricted language.
    #[test]
    fn g3_json_equivalent_rewrites_produce_identical_rows() {
        let compact = r#"[{"a":"café","b":"𝄞","n":-1.25e+2}]"#;
        let whitespace = " \n[ \t{ \"a\" : \"café\" , \"b\" : \"𝄞\" , \"n\" : -1.25e+2 } \r] \t";
        let escaped = r#"[{"n":-1.25e+2,"b":"\uD834\uDD1E","a":"caf\u00e9"}]"#;
        let expected = parse_rows(compact).expect("compact fixture");
        assert_eq!(
            parse_rows(whitespace).expect("whitespace rewrite"),
            expected,
            "RFC whitespace insertion must not move catalog semantics"
        );
        assert_eq!(
            parse_rows(escaped).expect("escape/member-order rewrite"),
            expected,
            "member permutation and equivalent Unicode escaping must agree"
        );
    }

    /// G3: no proper prefix of a valid document may publish a partial row;
    /// each truncation reports a byte offset inside or immediately after the
    /// available prefix.
    #[test]
    fn g3_json_all_truncation_prefixes_refuse_without_partial_results() {
        let complete = r#"[{"a":"\uD834\uDD1E","n":-1.25e+2},{"b":"escaped\ntext"}]"#;
        parse_rows(complete).expect("complete truncation fixture must parse");
        for cut in 0..complete.len() {
            match parse_rows(&complete[..cut]) {
                Err(IoError::Malformed { at, what }) => assert!(
                    at <= cut,
                    "truncation at {cut}: refusal offset {at} is outside prefix; detail={what:?}"
                ),
                other => panic!(
                    "truncation at byte {cut} must not publish a partial catalog; got {other:?}"
                ),
            }
        }
    }

    /// G0/G3: every logical resource dimension accepts its exact boundary and
    /// refuses the first excess before growing the corresponding payload.
    #[test]
    fn g0_json_resource_caps_are_exact_and_compositional() {
        let base = CatalogJsonLimits {
            max_input_bytes: 1024,
            max_rows: 8,
            max_members_per_object: 8,
            max_total_members: 16,
            max_string_bytes: 64,
            max_number_bytes: 64,
            max_decoded_bytes: 256,
        };

        let input = "[]";
        let exact = CatalogJsonLimits {
            max_input_bytes: input.len(),
            ..base
        };
        mini_json_array_of_objects(input, exact).expect("exact input-byte cap");
        assert_resource(
            input,
            CatalogJsonLimits {
                max_input_bytes: input.len() - 1,
                ..exact
            },
            "input-byte",
            "first input byte beyond cap",
        );

        let rows = "[{},{}]";
        mini_json_array_of_objects(
            rows,
            CatalogJsonLimits {
                max_rows: 2,
                ..base
            },
        )
        .expect("exact row cap");
        assert_resource(
            rows,
            CatalogJsonLimits {
                max_rows: 1,
                ..base
            },
            "row cap",
            "first row beyond cap",
        );
        match mini_json_array_of_objects(
            "[{},,]",
            CatalogJsonLimits {
                max_rows: 1,
                ..base
            },
        ) {
            Err(IoError::Malformed { at: 4, .. }) => {}
            other => panic!(
                "malformed token at exhausted row cap must remain a syntax refusal: {other:?}"
            ),
        }

        let members = r#"[{"a":"","b":""}]"#;
        mini_json_array_of_objects(
            members,
            CatalogJsonLimits {
                max_members_per_object: 2,
                ..base
            },
        )
        .expect("exact per-object member cap");
        assert_resource(
            members,
            CatalogJsonLimits {
                max_members_per_object: 1,
                ..base
            },
            "per-object member",
            "first object member beyond cap",
        );
        match mini_json_array_of_objects(
            r#"[{"a":"",,}]"#,
            CatalogJsonLimits {
                max_members_per_object: 1,
                ..base
            },
        ) {
            Err(IoError::Malformed { at, .. })
                if at == r#"[{"a":"",,}]"#.find(",,").expect("double comma") + 1 => {}
            other => panic!(
                "malformed token at exhausted member cap must remain a syntax refusal: {other:?}"
            ),
        }

        let aggregate_members = r#"[{"a":""},{"b":""}]"#;
        mini_json_array_of_objects(
            aggregate_members,
            CatalogJsonLimits {
                max_total_members: 2,
                ..base
            },
        )
        .expect("exact aggregate-member cap");
        assert_resource(
            aggregate_members,
            CatalogJsonLimits {
                max_total_members: 1,
                ..base
            },
            "aggregate member",
            "first aggregate member beyond cap",
        );

        let scalar = r#"[{"k":"\uD834\uDD1E"}]"#;
        for (spelling, case) in [(scalar, "escaped scalar"), (r#"[{"k":"𝄞"}]"#, "raw scalar")] {
            mini_json_array_of_objects(
                spelling,
                CatalogJsonLimits {
                    max_string_bytes: 4,
                    ..base
                },
            )
            .unwrap_or_else(|error| panic!("{case} at exact four-byte cap: {error:?}"));
            assert_resource(
                spelling,
                CatalogJsonLimits {
                    max_string_bytes: 3,
                    ..base
                },
                "string decoded-byte",
                &format!("{case} beyond string cap"),
            );
        }

        let number = r#"[{"n":-1.25e+2}]"#;
        mini_json_array_of_objects(
            number,
            CatalogJsonLimits {
                max_number_bytes: 8,
                ..base
            },
        )
        .expect("eight-byte number at exact token cap");
        assert_resource(
            number,
            CatalogJsonLimits {
                max_number_bytes: 7,
                ..base
            },
            "number-token byte",
            "first number byte beyond token cap",
        );

        let decoded = r#"[{"a":"bc"}]"#;
        mini_json_array_of_objects(
            decoded,
            CatalogJsonLimits {
                max_decoded_bytes: 3,
                ..base
            },
        )
        .expect("key plus value at exact aggregate decoded cap");
        assert_resource(
            decoded,
            CatalogJsonLimits {
                max_decoded_bytes: 2,
                ..base
            },
            "aggregate decoded-byte",
            "first decoded byte beyond aggregate cap",
        );
    }

    /// G3: at document end only the four RFC 8259 whitespace bytes are
    /// semantic-preserving; every other ASCII byte is trailing garbage.
    #[test]
    fn g3_json_trailing_ascii_matrix_only_accepts_json_whitespace() {
        let baseline = parse_rows("[]").expect("empty array baseline");
        for suffix in 0u8..=0x7f {
            let input = format!("[]{}", char::from(suffix));
            if matches!(suffix, b' ' | b'\n' | b'\r' | b'\t') {
                assert_eq!(
                    parse_rows(&input).unwrap_or_else(|error| panic!(
                        "JSON whitespace 0x{suffix:02x}: {error:?}"
                    )),
                    baseline,
                    "JSON whitespace suffix 0x{suffix:02x} must be inert"
                );
            } else {
                assert_malformed(
                    &input,
                    2,
                    "trailing bytes",
                    &format!("trailing ASCII byte 0x{suffix:02x}"),
                );
            }
        }
        let non_ascii_space = "[]\u{00a0}";
        assert_malformed(
            non_ascii_space,
            2,
            "trailing bytes",
            "non-JSON Unicode whitespace suffix",
        );
    }
}
