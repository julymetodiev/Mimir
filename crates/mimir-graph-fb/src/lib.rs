//! The graph on disk, as `graph.fbs` describes it. All the `unsafe` it takes to read bytes as
//! numbers stays in `generated`, behind a check of every offset, and no other crate needs any.

#[rustfmt::skip]
mod generated;
#[rustfmt::skip]
mod references_generated;

pub use flatbuffers::{FlatBufferBuilder, ForwardsUOffset, InvalidFlatbuffer, Vector, WIPOffset};
pub use generated::store::{
    Edge, File, FileArgs, Graph, GraphArgs, Name, NameArgs, Parameter, ParameterArgs, Range,
    Signature, SignatureArgs, Span, Symbol, SymbolArgs,
};
pub use references_generated::refs::{
    FileReferences, FileReferencesArgs, Place, Reference, ReferenceArgs, References, ReferencesArgs,
};
use self_cell::self_cell;

/// A vector of tables in the file.
pub type Tables<'a, T> = Vector<'a, ForwardsUOffset<T>>;

self_cell!(
    struct Verified {
        owner: Vec<u8>,

        #[covariant]
        dependent: Graph,
    }
);

self_cell!(
    struct VerifiedReferences {
        owner: Vec<u8>,

        #[covariant]
        dependent: References,
    }
);

/// The bytes of a file of references, every offset in them checked, with the root that reads them.
pub struct ReferencesBuffer(VerifiedReferences);

impl ReferencesBuffer {
    /// Check `bytes`. A project's references are many, so only offsets limit the check.
    pub fn verified(bytes: Vec<u8>) -> Result<Self, InvalidFlatbuffer> {
        let options = flatbuffers::VerifierOptions {
            max_tables: usize::MAX,
            max_apparent_size: usize::MAX,
            ..Default::default()
        };
        VerifiedReferences::try_new(bytes, |bytes| {
            references_generated::refs::root_as_references_with_opts(&options, bytes)
        })
        .map(Self)
    }

    /// The root of the references.
    pub fn references(&self) -> References<'_> {
        *self.0.borrow_dependent()
    }
}

/// A file's bytes, every offset in them checked, with the root that reads them.
pub struct GraphBuffer(Verified);

impl GraphBuffer {
    /// Check `bytes`. A graph is as large as its project, so only offsets limit the check.
    pub fn verified(bytes: Vec<u8>) -> Result<Self, InvalidFlatbuffer> {
        let options = flatbuffers::VerifierOptions {
            max_tables: usize::MAX,
            max_apparent_size: usize::MAX,
            ..Default::default()
        };
        Verified::try_new(bytes, |bytes| {
            generated::store::root_as_graph_with_opts(&options, bytes)
        })
        .map(Self)
    }

    /// The bytes it holds.
    pub fn bytes(&self) -> &[u8] {
        self.0.borrow_owner()
    }

    /// The root of the graph.
    pub fn graph(&self) -> Graph<'_> {
        *self.0.borrow_dependent()
    }
}
