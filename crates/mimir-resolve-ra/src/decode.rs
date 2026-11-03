//! The SCIP format, which is protobuf: only the fields Mimir reads.

use std::path::PathBuf;

use mimir_resolve::{Occurrence, SemanticIndex};

/// Decode what is needed of `scip.proto`: `Index.documents = 2`; `Document.relative_path = 1`,
/// `occurrences = 2`; `Occurrence.range = 1`, `symbol = 2`, `symbol_roles = 3`.
pub fn decode(bytes: &[u8]) -> Result<SemanticIndex, String> {
    let mut index = SemanticIndex::default();
    for field in Fields::new(bytes) {
        if let (2, Value::Bytes(document)) = field? {
            let mut path = None;
            let mut occurrences = Vec::new();
            for field in Fields::new(document) {
                match field? {
                    (1, Value::Bytes(p)) => {
                        path = Some(PathBuf::from(String::from_utf8_lossy(p).into_owned()))
                    }
                    (2, Value::Bytes(o)) => occurrences.push(decode_occurrence(o)?),
                    _ => {}
                }
            }
            let path = path.ok_or("document without a path")?;
            index.documents.insert(path, occurrences);
        }
    }
    Ok(index)
}

fn decode_occurrence(bytes: &[u8]) -> Result<Occurrence, String> {
    let (mut range, mut symbol, mut roles) = (Vec::new(), String::new(), 0);
    for field in Fields::new(bytes) {
        match field? {
            (1, Value::Bytes(packed)) => {
                let mut at = 0;
                while at < packed.len() {
                    range.push(varint(packed, &mut at)? as u32);
                }
            }
            (1, Value::Varint(v)) => range.push(v as u32),
            (2, Value::Bytes(s)) => symbol = String::from_utf8_lossy(s).into_owned(),
            (3, Value::Varint(v)) => roles = v,
            _ => {}
        }
    }
    // [line, column, end_column], or [line, column, end_line, end_column].
    let (line, column, end_line, end_column) = match range[..] {
        [l, c, ec] => (l, c, l, ec),
        [l, c, el, ec] => (l, c, el, ec),
        _ => return Err(format!("bad occurrence range {range:?}")),
    };
    Ok(Occurrence {
        line,
        column,
        end_line,
        end_column,
        symbol,
        roles,
    })
}

enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

/// The fields of a protobuf message.
struct Fields<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Fields<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn read(&mut self) -> Result<(u64, Value<'a>), String> {
        let key = varint(self.bytes, &mut self.at)?;
        let value = match key & 7 {
            0 => Value::Varint(varint(self.bytes, &mut self.at)?),
            1 => self.skip(8)?,
            2 => {
                let len = varint(self.bytes, &mut self.at)? as usize;
                let end = self
                    .at
                    .checked_add(len)
                    .filter(|e| *e <= self.bytes.len())
                    .ok_or("truncated field")?;
                let value = &self.bytes[self.at..end];
                self.at = end;
                Value::Bytes(value)
            }
            5 => self.skip(4)?,
            wire => return Err(format!("unsupported wire type {wire}")),
        };
        Ok((key >> 3, value))
    }

    fn skip(&mut self, n: usize) -> Result<Value<'a>, String> {
        if self.at + n > self.bytes.len() {
            return Err("truncated field".to_string());
        }
        self.at += n;
        Ok(Value::Fixed)
    }
}

impl<'a> Iterator for Fields<'a> {
    type Item = Result<(u64, Value<'a>), String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.at >= self.bytes.len() {
            return None;
        }
        let item = self.read();
        if item.is_err() {
            self.at = self.bytes.len();
        }
        Some(item)
    }
}

fn varint(bytes: &[u8], at: &mut usize) -> Result<u64, String> {
    let (mut value, mut shift) = (0u64, 0);
    loop {
        let byte = *bytes.get(*at).ok_or("truncated varint")?;
        *at += 1;
        value |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return Ok(value);
        }
        shift += 7;
        if shift > 63 {
            return Err("varint too long".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn field(number: u8, bytes: &[u8]) -> Vec<u8> {
        let mut out = vec![number << 3 | 2, bytes.len() as u8];
        out.extend_from_slice(bytes);
        out
    }

    #[test]
    fn decodes_three_and_four_part_ranges() {
        let short = [field(1, &[3, 4, 9]), field(2, b"a()."), vec![3 << 3, 1]].concat();
        let long = [field(1, &[3, 4, 5, 2]), field(2, b"b().")].concat();
        let document = [field(1, b"src/a.rs"), field(2, &short), field(2, &long)].concat();
        let index = decode(&field(2, &document)).unwrap();

        let occurrences = &index.documents[Path::new("src/a.rs")];
        assert_eq!(occurrences[0].end_line, 3);
        assert_eq!(occurrences[0].end_column, 9);
        assert_eq!(occurrences[0].roles, 1);
        assert_eq!(occurrences[1].end_line, 5);
        assert_eq!(occurrences[1].symbol, "b().");
    }

    #[test]
    fn refuses_truncated_input() {
        assert!(decode(&[2 << 3 | 2, 200]).is_err());
    }
}
