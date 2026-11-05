//! The graph as bytes for a file. Nothing here touches the disk.

use crate::file::{FileEntry, position_of};
use crate::format::{
    BUILT_BY, SharedStrings, VERSION, confidence_number, edge_number, kind_number, language_number,
    visibility_number,
};
use crate::relations::EdgeData;
use latt_graph_fb as fb;
use latt_types::{ContentHash, FileId, Signature, Symbol, SymbolId};
use std::collections::{BTreeMap, HashMap};

/// The stamp of symbols with these ids, sorted: what ties edges to the positions they are in.
fn stamp_of(ids: impl Iterator<Item = u64>) -> u64 {
    let bytes: Vec<u8> = ids.flat_map(u64::to_le_bytes).collect();
    let hash = ContentHash::compute(&bytes);
    let mut stamp = [0; 8];
    stamp.copy_from_slice(&hash.as_bytes()[..8]);
    u64::from_le_bytes(stamp)
}

/// The bytes of a file holding these edges, between the symbols of a graph. An edge whose end is not
/// among them is left out.
pub fn encode_edges(graph: fb::Graph<'_>, all_edges: &[(SymbolId, SymbolId, EdgeData)]) -> Vec<u8> {
    let Some(symbols) = graph.symbols() else {
        return Vec::new();
    };
    let mut edges: Vec<fb::Edge> = all_edges
        .iter()
        .filter_map(|(from, to, edge)| {
            Some(fb::Edge::new(
                position_of(symbols, *from)? as u32,
                position_of(symbols, *to)? as u32,
                edge.span.start,
                edge.span.end,
                edge_number(edge.kind),
                confidence_number(edge.confidence),
                0,
            ))
        })
        .collect();
    edges.sort_by_key(|edge| {
        (
            edge.from(),
            edge.to(),
            edge.start(),
            edge.end(),
            edge.kind(),
            edge.confidence(),
        )
    });
    let count = symbols.len();
    let mut out_offsets = vec![0u32; count + 1];
    let mut in_offsets = vec![0u32; count + 1];
    for edge in &edges {
        out_offsets[edge.from() as usize + 1] += 1;
        in_offsets[edge.to() as usize + 1] += 1;
    }
    for position in 0..count {
        out_offsets[position + 1] += out_offsets[position];
        in_offsets[position + 1] += in_offsets[position];
    }
    let mut next = in_offsets.clone();
    let mut incoming = vec![0u32; edges.len()];
    for (at, edge) in edges.iter().enumerate() {
        let place = &mut next[edge.to() as usize];
        incoming[*place as usize] = at as u32;
        *place += 1;
    }

    let mut builder = fb::FlatBufferBuilder::with_capacity(1 << 16);
    let edges = builder.create_vector(&edges);
    let out_offsets = builder.create_vector(&out_offsets);
    let incoming = builder.create_vector(&incoming);
    let in_offsets = builder.create_vector(&in_offsets);
    let built_by = builder.create_string(BUILT_BY);
    let root = fb::Edges::create(
        &mut builder,
        &fb::EdgesArgs {
            version: VERSION,
            built_by: Some(built_by),
            stamp: graph.stamp(),
            edges: Some(edges),
            out_offsets: Some(out_offsets),
            incoming: Some(incoming),
            in_offsets: Some(in_offsets),
        },
    );
    builder.finish(root, None);
    builder.finished_data().to_vec()
}

/// The bytes of a file holding these symbols and files.
pub fn encode_symbols(mut symbols: Vec<Symbol>, files: &[FileEntry]) -> Vec<u8> {
    // Ids are hashes, so the order the symbols came in is kept apart from theirs: which of two
    // symbols on one place holds an edge depends on it.
    let source_order: Vec<SymbolId> = symbols.iter().map(|symbol| symbol.id).collect();
    symbols.sort_by_key(|symbol| symbol.id.value());
    let positions: HashMap<SymbolId, u32> = symbols
        .iter()
        .enumerate()
        .map(|(position, symbol)| (symbol.id, position as u32))
        .collect();

    let mut names: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    let mut qualified: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut in_file: HashMap<FileId, Vec<u32>> = HashMap::new();
    for id in &source_order {
        let position = positions[id];
        let symbol = &symbols[position as usize];
        names.entry(&symbol.name).or_default().push(position);
        qualified
            .entry(symbol.qualified_name.join("::"))
            .or_default()
            .push(position);
        in_file.entry(symbol.file_id).or_default().push(position);
    }
    let stamp = stamp_of(symbols.iter().map(|symbol| symbol.id.value()));

    let mut builder = fb::FlatBufferBuilder::with_capacity(1 << 20);
    let mut strings = SharedStrings::new();
    let symbol_tables: Vec<_> = symbols
        .iter()
        .map(|symbol| write_symbol(&mut builder, &mut strings, symbol))
        .collect();
    let symbols_vector = builder.create_vector(&symbol_tables);
    let names = write_names(&mut builder, names.iter().map(|(name, at)| (*name, at)));
    let qualified = write_names(
        &mut builder,
        qualified.iter().map(|(name, at)| (name.as_str(), at)),
    );

    let mut sorted_files: Vec<&FileEntry> = files.iter().collect();
    sorted_files.sort_by_key(|file| file.id.value());
    let file_tables: Vec<_> = sorted_files
        .iter()
        .map(|file| {
            let path = builder.create_string(&file.path.to_string_lossy());
            let hash = builder.create_vector(file.hash.as_bytes().as_slice());
            let at = in_file.get(&file.id).map_or(&[][..], Vec::as_slice);
            let at = builder.create_vector(at);
            fb::File::create(
                &mut builder,
                &fb::FileArgs {
                    path: Some(path),
                    id: file.id.value(),
                    hash: Some(hash),
                    size: file.size,
                    modified: file.modified,
                    symbols: Some(at),
                },
            )
        })
        .collect();
    let files_vector = builder.create_vector(&file_tables);
    let built_by = builder.create_string(BUILT_BY);
    let root = fb::Graph::create(
        &mut builder,
        &fb::GraphArgs {
            version: VERSION,
            built_by: Some(built_by),
            symbols: Some(symbols_vector),
            names: Some(names),
            qualified: Some(qualified),
            files: Some(files_vector),
            stamp,
        },
    );
    builder.finish(root, None);
    builder.finished_data().to_vec()
}

fn write_symbol<'s, 'a>(
    builder: &mut fb::FlatBufferBuilder<'a>,
    strings: &mut SharedStrings<'s, 'a>,
    symbol: &'s Symbol,
) -> fb::WIPOffset<fb::Symbol<'a>> {
    let name = strings.get(builder, &symbol.name);
    let segments: Vec<_> = symbol
        .qualified_name
        .segments()
        .iter()
        .map(|segment| strings.get(builder, segment))
        .collect();
    let segments = builder.create_vector(&segments);
    let signature = symbol
        .signature
        .as_ref()
        .map(|signature| write_signature(builder, strings, signature));
    let doc = symbol.doc.as_ref().map(|doc| builder.create_string(doc));
    fb::Symbol::create(
        builder,
        &fb::SymbolArgs {
            id: symbol.id.value(),
            name: Some(name),
            qualified: Some(segments),
            kind: kind_number(symbol.kind),
            language: language_number(symbol.language),
            file: symbol.file_id.value(),
            visibility: visibility_number(symbol.visibility),
            range: Some(&fb::Range::new(
                symbol.range.start_line,
                symbol.range.start_col,
                symbol.range.end_line,
                symbol.range.end_col,
            )),
            span: Some(&fb::Span::new(symbol.span.start, symbol.span.end)),
            signature,
            doc,
        },
    )
}

fn write_signature<'s, 'a>(
    builder: &mut fb::FlatBufferBuilder<'a>,
    strings: &mut SharedStrings<'s, 'a>,
    signature: &'s Signature,
) -> fb::WIPOffset<fb::Signature<'a>> {
    let parameters: Vec<_> = signature
        .parameters
        .iter()
        .map(|parameter| {
            let name = strings.get(builder, &parameter.name);
            let annotation = parameter
                .type_annotation
                .as_ref()
                .map(|text| strings.get(builder, text));
            let default = parameter
                .default_value
                .as_ref()
                .map(|text| builder.create_string(text));
            fb::Parameter::create(
                builder,
                &fb::ParameterArgs {
                    name: Some(name),
                    type_annotation: annotation,
                    default_value: default,
                },
            )
        })
        .collect();
    let parameters = builder.create_vector(&parameters);
    let return_type = signature
        .return_type
        .as_ref()
        .map(|text| strings.get(builder, text));
    let type_parameters: Vec<_> = signature
        .type_parameters
        .iter()
        .map(|text| strings.get(builder, text))
        .collect();
    let type_parameters = builder.create_vector(&type_parameters);
    let raw = builder.create_string(&signature.raw);
    fb::Signature::create(
        builder,
        &fb::SignatureArgs {
            parameters: Some(parameters),
            return_type,
            type_parameters: Some(type_parameters),
            raw: Some(raw),
        },
    )
}

fn write_names<'a, 'n>(
    builder: &mut fb::FlatBufferBuilder<'a>,
    names: impl Iterator<Item = (&'n str, &'n Vec<u32>)>,
) -> fb::WIPOffset<fb::Tables<'a, fb::Name<'a>>> {
    let tables: Vec<_> = names
        .map(|(name, at)| {
            let name = builder.create_string(name);
            let at = builder.create_vector(at);
            fb::Name::create(
                builder,
                &fb::NameArgs {
                    name: Some(name),
                    symbols: Some(at),
                },
            )
        })
        .collect();
    builder.create_vector(&tables)
}
