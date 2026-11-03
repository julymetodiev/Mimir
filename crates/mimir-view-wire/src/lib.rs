//! What the page of `mimir view` is sent, as raw arrays that go into GPU buffers as they are
//! (docs/ARCHITECTURE.md section 7). Plain bytes: the page runs in a browser, without the wire of
//! the daemon, which is made for a socket.

use thiserror::Error;

/// A message that makes no sense.
#[derive(Debug, Error, PartialEq)]
pub enum DecodeError {
    #[error("the message ends too early")]
    TooShort,
    #[error("the message has an unknown kind {0}")]
    UnknownKind(u8),
    #[error("a name in the message is not text")]
    NotText,
}

/// What a symbol is, for its colour.
pub const KIND_FUNCTION: u8 = 0;
pub const KIND_TYPE: u8 = 1;
pub const KIND_MEMBER: u8 = 2;
pub const KIND_CONSTANT: u8 = 3;
pub const KIND_OTHER: u8 = 4;

/// What an edge is.
pub const EDGE_CALL: u32 = 0;
pub const EDGE_POSSIBLE: u32 = 1;
pub const EDGE_USE: u32 = 2;
pub const EDGE_IMPORT: u32 = 3;

/// The whole graph.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub generation: u64,
    pub provisional: bool,
    pub files: Vec<String>,
    pub ids: Vec<u64>,
    pub names: Vec<String>,
    pub kinds: Vec<u8>,
    /// Each symbol's place in `files`.
    pub symbol_files: Vec<u32>,
    pub lines: Vec<u32>,
    /// From, to and kind of each edge, the ends by their places in the symbols.
    pub edges: Vec<[u32; 3]>,
}

/// What the server tells the page.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    Snapshot(Snapshot),
    /// A query or, with `edit`, an edit touched these symbols.
    Activity {
        symbols: Vec<u64>,
        edit: bool,
    },
    /// The source of a symbol the page asked for.
    Source {
        id: u64,
        first_line: u32,
        text: String,
    },
}

/// What the page asks the server. It has no way to ask for a change.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    Source { id: u64 },
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn text(&mut self, text: &str) {
        self.u32(text.len() as u32);
        self.0.extend_from_slice(text.as_bytes());
    }
    fn texts(&mut self, texts: &[String]) {
        self.u32(texts.len() as u32);
        for text in texts {
            self.text(text);
        }
    }
    fn u32s(&mut self, values: &[u32]) {
        self.u32(values.len() as u32);
        for value in values {
            self.u32(*value);
        }
    }
    fn u64s(&mut self, values: &[u64]) {
        self.u32(values.len() as u32);
        for value in values {
            self.u64(*value);
        }
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], DecodeError> {
        if self.0.len() < count {
            return Err(DecodeError::TooShort);
        }
        let (taken, rest) = self.0.split_at(count);
        self.0 = rest;
        Ok(taken)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        let low = u64::from(self.u32()?);
        let high = u64::from(self.u32()?);
        Ok(high << 32 | low)
    }
    fn text(&mut self) -> Result<String, DecodeError> {
        let length = self.u32()? as usize;
        let bytes = self.take(length)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::NotText)
    }
    /// A count from the message, refused if the message cannot hold that many of the smallest item.
    fn count(&mut self, smallest: usize) -> Result<usize, DecodeError> {
        let count = self.u32()? as usize;
        if self.0.len() / smallest < count {
            return Err(DecodeError::TooShort);
        }
        Ok(count)
    }
    fn texts(&mut self) -> Result<Vec<String>, DecodeError> {
        let count = self.count(4)?;
        (0..count).map(|_| self.text()).collect()
    }
    fn u32s(&mut self) -> Result<Vec<u32>, DecodeError> {
        let count = self.count(4)?;
        (0..count).map(|_| self.u32()).collect()
    }
    fn u64s(&mut self) -> Result<Vec<u64>, DecodeError> {
        let count = self.count(8)?;
        (0..count).map(|_| self.u64()).collect()
    }
}

const SNAPSHOT: u8 = 0;
const ACTIVITY: u8 = 1;
const SOURCE: u8 = 2;

impl ServerMessage {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Writer(Vec::new());
        match self {
            Self::Snapshot(snapshot) => {
                out.0.push(SNAPSHOT);
                out.u64(snapshot.generation);
                out.0.push(u8::from(snapshot.provisional));
                out.texts(&snapshot.files);
                out.u64s(&snapshot.ids);
                out.texts(&snapshot.names);
                out.u32(snapshot.kinds.len() as u32);
                out.0.extend_from_slice(&snapshot.kinds);
                out.u32s(&snapshot.symbol_files);
                out.u32s(&snapshot.lines);
                out.u32(snapshot.edges.len() as u32);
                for edge in &snapshot.edges {
                    edge.iter().for_each(|value| out.u32(*value));
                }
            }
            Self::Activity { symbols, edit } => {
                out.0.push(ACTIVITY);
                out.0.push(u8::from(*edit));
                out.u64s(symbols);
            }
            Self::Source {
                id,
                first_line,
                text,
            } => {
                out.0.push(SOURCE);
                out.u64(*id);
                out.u32(*first_line);
                out.text(text);
            }
        }
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut input = Reader(bytes);
        match input.u8()? {
            SNAPSHOT => {
                let generation = input.u64()?;
                let provisional = input.u8()? != 0;
                let files = input.texts()?;
                let ids = input.u64s()?;
                let names = input.texts()?;
                let kinds_length = input.u32()? as usize;
                let kinds = input.take(kinds_length)?.to_vec();
                let symbol_files = input.u32s()?;
                let lines = input.u32s()?;
                let edge_count = input.count(12)?;
                let mut edges = Vec::with_capacity(edge_count);
                for _ in 0..edge_count {
                    edges.push([input.u32()?, input.u32()?, input.u32()?]);
                }
                Ok(Self::Snapshot(Snapshot {
                    generation,
                    provisional,
                    files,
                    ids,
                    names,
                    kinds,
                    symbol_files,
                    lines,
                    edges,
                }))
            }
            ACTIVITY => {
                let edit = input.u8()? != 0;
                Ok(Self::Activity {
                    symbols: input.u64s()?,
                    edit,
                })
            }
            SOURCE => Ok(Self::Source {
                id: input.u64()?,
                first_line: input.u32()?,
                text: input.text()?,
            }),
            other => Err(DecodeError::UnknownKind(other)),
        }
    }
}

impl ClientMessage {
    pub fn encode(&self) -> Vec<u8> {
        let Self::Source { id } = self;
        id.to_le_bytes().to_vec()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        Ok(Self::Source {
            id: Reader(bytes).u64()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot {
            generation: 7,
            provisional: true,
            files: vec!["a.rs".to_string()],
            ids: vec![u64::MAX, 2],
            names: vec!["caller".to_string(), "имe".to_string()],
            kinds: vec![KIND_FUNCTION, KIND_TYPE],
            symbol_files: vec![0, 0],
            lines: vec![1, 2],
            edges: vec![[0, 1, EDGE_CALL]],
        }
    }

    #[test]
    fn test_every_message_comes_back_as_it_went() {
        for message in [
            ServerMessage::Snapshot(snapshot()),
            ServerMessage::Activity {
                symbols: vec![1, 2],
                edit: true,
            },
            ServerMessage::Source {
                id: 9,
                first_line: 3,
                text: "fn a() {}\n".to_string(),
            },
        ] {
            assert_eq!(ServerMessage::decode(&message.encode()), Ok(message));
        }
        let asked = ClientMessage::Source { id: u64::MAX };
        assert_eq!(ClientMessage::decode(&asked.encode()), Ok(asked));
    }

    #[test]
    fn test_a_message_cut_anywhere_is_refused_and_does_not_panic() {
        let whole = ServerMessage::Snapshot(snapshot()).encode();
        for length in 0..whole.len() {
            assert!(ServerMessage::decode(&whole[..length]).is_err(), "{length}");
        }
        assert_eq!(
            ServerMessage::decode(&[9]),
            Err(DecodeError::UnknownKind(9))
        );
    }

    #[test]
    fn test_a_count_larger_than_the_message_is_refused_before_it_allocates() {
        let mut lying = vec![ACTIVITY, 0];
        lying.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(ServerMessage::decode(&lying), Err(DecodeError::TooShort));
    }
}
