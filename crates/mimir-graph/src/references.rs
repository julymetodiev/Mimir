//! The references of every file as bytes for a file, with a hash of the text each came from, so that
//! a link parses only what changed. Nothing here touches the disk.

use crate::file::FileError;
use crate::format::{confidence_number, confidence_of};
use mimir_graph_fb as fb;
use mimir_types::{CallForm, ContentHash, ExtractedReference, FileId, Range, ReferenceKind, Span};

/// Another version of the format is treated as no file.
const VERSION: u32 = 1;

/// Another Mimir may parse otherwise, so its references are treated as no file too.
const BUILT_BY: &str = env!("CARGO_PKG_VERSION");

fn kind_number(kind: ReferenceKind) -> u8 {
    match kind {
        ReferenceKind::Call => 0,
        ReferenceKind::FunctionValue => 1,
        ReferenceKind::Import => 2,
        ReferenceKind::Read => 3,
        ReferenceKind::FormatRead => 4,
        ReferenceKind::LocalRead => 5,
        ReferenceKind::TypeUse => 6,
        ReferenceKind::FieldUse => 7,
        ReferenceKind::FieldWrite => 8,
        ReferenceKind::MethodDeclaration => 9,
    }
}

fn kind_of(number: u8) -> Option<ReferenceKind> {
    Some(match number {
        0 => ReferenceKind::Call,
        1 => ReferenceKind::FunctionValue,
        2 => ReferenceKind::Import,
        3 => ReferenceKind::Read,
        4 => ReferenceKind::FormatRead,
        5 => ReferenceKind::LocalRead,
        6 => ReferenceKind::TypeUse,
        7 => ReferenceKind::FieldUse,
        8 => ReferenceKind::FieldWrite,
        9 => ReferenceKind::MethodDeclaration,
        _ => return None,
    })
}

fn form_number(form: Option<CallForm>) -> u8 {
    match form {
        None => 0,
        Some(CallForm::Plain) => 1,
        Some(CallForm::Method) => 2,
        Some(CallForm::Scoped) => 3,
        Some(CallForm::Shorthand) => 4,
        Some(CallForm::Bound) => 5,
    }
}

/// The form of a call, or `None` if the number means none of them.
fn form_of(number: u8) -> Option<Option<CallForm>> {
    Some(match number {
        0 => None,
        1 => Some(CallForm::Plain),
        2 => Some(CallForm::Method),
        3 => Some(CallForm::Scoped),
        4 => Some(CallForm::Shorthand),
        5 => Some(CallForm::Bound),
        _ => return None,
    })
}

/// The bytes of a file holding the references of these files, each with a hash of its text.
pub fn encode_references(files: &[(FileId, ContentHash, &[ExtractedReference])]) -> Vec<u8> {
    let mut sorted: Vec<&(FileId, ContentHash, &[ExtractedReference])> = files.iter().collect();
    sorted.sort_by_key(|(id, _, _)| id.value());
    let mut builder = fb::FlatBufferBuilder::with_capacity(1 << 20);
    let tables: Vec<_> = sorted
        .iter()
        .map(|(id, hash, references)| {
            let references: Vec<_> = references
                .iter()
                .map(|reference| {
                    let name = builder.create_shared_string(&reference.name);
                    let context = reference
                        .context
                        .as_ref()
                        .map(|text| builder.create_shared_string(text));
                    let container = reference
                        .container
                        .as_ref()
                        .map(|text| builder.create_shared_string(text));
                    let (receiver_line, receiver_col) = reference.receiver_at.unwrap_or((0, 0));
                    fb::Reference::create(
                        &mut builder,
                        &fb::ReferenceArgs {
                            name: Some(name),
                            kind: kind_number(reference.kind),
                            place: Some(&fb::Place::new(
                                reference.range.start_line,
                                reference.range.start_col,
                                reference.range.end_line,
                                reference.range.end_col,
                                reference.span.start,
                                reference.span.end,
                            )),
                            confidence: confidence_number(reference.confidence),
                            context,
                            container,
                            call_form: form_number(reference.call_form),
                            receiver_line,
                            receiver_col,
                        },
                    )
                })
                .collect();
            let references = builder.create_vector(&references);
            let hash = builder.create_vector(hash.as_bytes().as_slice());
            fb::FileReferences::create(
                &mut builder,
                &fb::FileReferencesArgs {
                    id: id.value(),
                    hash: Some(hash),
                    references: Some(references),
                },
            )
        })
        .collect();
    let files = builder.create_vector(&tables);
    let built_by = builder.create_string(BUILT_BY);
    let root = fb::References::create(
        &mut builder,
        &fb::ReferencesArgs {
            version: VERSION,
            built_by: Some(built_by),
            files: Some(files),
        },
    );
    builder.finish(root, None);
    builder.finished_data().to_vec()
}

/// References read from a file.
pub struct ReferencesFile(fb::ReferencesBuffer);

impl ReferencesFile {
    /// Check `bytes` as a whole, and take them.
    pub fn open(bytes: Vec<u8>) -> Result<Self, FileError> {
        let buffer = fb::ReferencesBuffer::verified(bytes).map_err(FileError::Invalid)?;
        let root = buffer.references();
        if root.version() != VERSION || root.built_by() != Some(BUILT_BY) {
            return Err(FileError::Version(root.version()));
        }
        if root.files().is_none() {
            return Err(FileError::Broken("no files"));
        }
        Ok(Self(buffer))
    }

    /// The references of a file, if they came from text with this hash. A file changed since, or a
    /// reference that cannot be read, gives nothing: the file is then parsed.
    pub fn references_of(
        &self,
        file: FileId,
        hash: ContentHash,
    ) -> Option<Vec<ExtractedReference>> {
        let files = self.0.references().files()?;
        let entry =
            files.lookup_by_key(file.value(), |entry, id| entry.key_compare_with_value(*id))?;
        if entry.hash()?.bytes() != hash.as_bytes() {
            return None;
        }
        entry.references()?.iter().map(read_reference).collect()
    }
}

fn read_reference(reference: fb::Reference<'_>) -> Option<ExtractedReference> {
    let place = reference.place()?;
    if place.start() > place.end() {
        return None;
    }
    Some(ExtractedReference {
        name: reference.name()?.to_string(),
        kind: kind_of(reference.kind())?,
        range: Range {
            start_line: place.start_line(),
            start_col: place.start_col(),
            end_line: place.end_line(),
            end_col: place.end_col(),
        },
        span: Span::new(place.start(), place.end()),
        confidence: confidence_of(reference.confidence())?,
        context: reference.context().map(str::to_string),
        container: reference.container().map(str::to_string),
        call_form: form_of(reference.call_form())?,
        receiver_at: (reference.receiver_line() != 0)
            .then(|| (reference.receiver_line(), reference.receiver_col())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_types::Confidence;

    fn sample() -> (Vec<ExtractedReference>, ContentHash, Vec<u8>) {
        let plain = ExtractedReference::new(
            "plain".to_string(),
            ReferenceKind::Import,
            Range {
                start_line: 1,
                start_col: 2,
                end_line: 3,
                end_col: 4,
            },
            Span::new(5, 6),
        );
        let full = ExtractedReference::new(
            "full".to_string(),
            ReferenceKind::Call,
            plain.range,
            Span::new(7, 8),
        )
        .with_confidence(Confidence::Low)
        .with_context("a::b".to_string())
        .with_container("holder".to_string())
        .with_call_form(CallForm::Scoped)
        .with_receiver_at(Some((9, 10)));
        let references = vec![plain, full];
        let hash = ContentHash::compute(b"text");
        let bytes = encode_references(&[
            (FileId::new(2), ContentHash::compute(b"other"), &[]),
            (FileId::new(1), hash, &references),
        ]);
        (references, hash, bytes)
    }

    #[test]
    fn what_goes_in_comes_out() {
        let (references, hash, bytes) = sample();
        let file = ReferencesFile::open(bytes).unwrap();
        let read = file.references_of(FileId::new(1), hash).unwrap();
        assert_eq!(format!("{read:?}"), format!("{references:?}"));
        assert!(
            file.references_of(FileId::new(2), ContentHash::compute(b"other"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_changed_file_gets_nothing() {
        let (_, _, bytes) = sample();
        let file = ReferencesFile::open(bytes).unwrap();
        let other = ContentHash::compute(b"changed");
        assert!(file.references_of(FileId::new(1), other).is_none());
        assert!(file.references_of(FileId::new(3), other).is_none());
    }

    #[test]
    fn broken_bytes_are_refused_and_never_stop_it() {
        let (_, hash, bytes) = sample();
        assert!(ReferencesFile::open(Vec::new()).is_err());
        assert!(ReferencesFile::open(bytes[..bytes.len() / 2].to_vec()).is_err());
        for at in 0..bytes.len() {
            let mut changed = bytes.clone();
            changed[at] ^= 0xff;
            if let Ok(file) = ReferencesFile::open(changed) {
                file.references_of(FileId::new(1), hash);
            }
        }
    }

    #[test]
    fn numbers_go_both_ways() {
        for number in 0..=u8::MAX {
            if let Some(kind) = kind_of(number) {
                assert_eq!(kind_number(kind), number);
            }
            if let Some(confidence) = confidence_of(number) {
                assert_eq!(confidence_number(confidence), number);
            }
            if let Some(form) = form_of(number) {
                assert_eq!(form_number(form), number);
            }
        }
    }
}
