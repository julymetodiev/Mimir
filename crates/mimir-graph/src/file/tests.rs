use super::*;
use crate::format::{
    confidence_number, edge_number, kind_number, language_number, visibility_number,
};
use crate::relations::EdgeKind;
use mimir_types::{Confidence, Language, SymbolKind, Visibility};

fn symbol(id: u64, name: &str, file: u64) -> Symbol {
    Symbol {
        id: SymbolId::new(id),
        name: name.to_string(),
        qualified_name: QualifiedName::new(vec!["m".to_string(), name.to_string()]),
        kind: SymbolKind::Function,
        language: Language::Cpp,
        file_id: FileId::new(file),
        range: Range {
            start_line: 1,
            start_col: 2,
            end_line: 3,
            end_col: 4,
        },
        span: Span::new(5, 6),
        signature: None,
        visibility: Visibility::Public,
        doc: None,
    }
}

fn sample() -> GraphFile {
    let mut with_all = symbol(30, "with_all", 1);
    with_all.signature = Some(Signature {
        parameters: vec![
            Parameter {
                name: "a".to_string(),
                type_annotation: Some("int".to_string()),
                default_value: Some("1".to_string()),
            },
            Parameter {
                name: "b".to_string(),
                type_annotation: None,
                default_value: None,
            },
        ],
        return_type: Some("int".to_string()),
        type_parameters: vec!["T".to_string()],
        raw: "int with_all(int a = 1, b)".to_string(),
    });
    with_all.doc = Some("It does all.".to_string());
    let symbols = vec![
        with_all,
        symbol(10, "same", 1),
        symbol(20, "same", 2),
        symbol(40, "alone", 2),
    ];
    let edge = |kind, start| EdgeData::new(kind, Span::new(start, start + 1), Confidence::High);
    let (a, b, c) = (SymbolId::new(30), SymbolId::new(10), SymbolId::new(20));
    let edges = vec![
        (a, b, edge(EdgeKind::Calls, 1)),
        (a, b, edge(EdgeKind::UsesAsValue, 2)),
        (a, c, edge(EdgeKind::Calls, 3)),
        (c, a, edge(EdgeKind::Implements, 4)),
        (a, SymbolId::new(99), edge(EdgeKind::Calls, 5)),
    ];
    let files = [("a.cpp", 1), ("b.cpp", 2), ("empty.cpp", 3)].map(|(path, id)| FileEntry {
        id: FileId::new(id),
        path: PathBuf::from(path),
        hash: ContentHash::compute(path.as_bytes()),
        size: 7,
        modified: 8,
    });
    GraphFile::build(symbols, &edges, &files).unwrap()
}

#[test]
fn what_goes_in_comes_out() {
    let file = sample();
    assert_eq!((file.symbol_count(), file.edge_count()), (4, 4));
    assert_eq!(file.find_symbol("same").len(), 2);
    assert!(file.find_symbol("none").is_empty());
    assert_eq!(file.find_by_qualified_name("m::alone").len(), 1);
    assert_eq!(file.symbols_in_file(FileId::new(1)).len(), 2);
    assert_eq!(file.count_in_file(FileId::new(2)), 2);
    assert_eq!(file.count_in_file(FileId::new(3)), 0);
    let kept = file.get_symbol(SymbolId::new(30)).unwrap();
    assert_eq!(kept.doc.as_deref(), Some("It does all."));
    assert_eq!(kept.qualified_name.join("::"), "m::with_all");
    let signature = kept.signature.unwrap();
    assert_eq!(signature.parameters[0].default_value.as_deref(), Some("1"));
    assert_eq!(signature.parameters[1].type_annotation, None);
    assert!(file.get_symbol(SymbolId::new(99)).is_none());
    assert_eq!(file.outgoing(SymbolId::new(30)).len(), 3);
    assert_eq!(file.incoming(SymbolId::new(10)).len(), 2);
    assert!(file.outgoing(SymbolId::new(99)).is_empty());
    let paths: Vec<_> = file
        .files()
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    assert_eq!(paths.len(), 3);
    assert!(paths.contains(&PathBuf::from("empty.cpp")));
    assert_eq!(file.files()[0].size, 7);
    assert_eq!(file.files()[0].modified, 8);
    let again = GraphFile::open(file.symbol_bytes().to_vec(), file.edge_bytes().to_vec()).unwrap();
    assert_eq!((again.symbol_count(), again.edge_count()), (4, 4));
}

#[test]
fn new_edges_write_no_symbol() {
    let file = sample();
    let edge = EdgeData::new(EdgeKind::Calls, Span::new(1, 2), Confidence::High);
    let linked = file
        .with_edges(&[(SymbolId::new(10), SymbolId::new(20), edge)])
        .unwrap();
    assert!(linked.has_symbols_of(&file));
    assert_eq!((linked.symbol_count(), linked.edge_count()), (4, 1));
    assert_eq!(linked.outgoing(SymbolId::new(10)).len(), 1);
    assert!(linked.outgoing(SymbolId::new(30)).is_empty());
}

#[test]
fn edges_of_other_symbols_are_refused() {
    let file = sample();
    let other = GraphFile::build(vec![symbol(5, "other", 1)], &[], &[]).unwrap();
    assert!(GraphFile::open(file.symbol_bytes().to_vec(), other.edge_bytes().to_vec()).is_err());
    let alone = GraphFile::open_without_edges(file.symbol_bytes().to_vec()).unwrap();
    assert_eq!((alone.symbol_count(), alone.edge_count()), (4, 0));
}

#[test]
fn a_file_keeps_the_order_its_symbols_came_in_whatever_their_ids() {
    let symbols = vec![
        symbol(30, "first", 1),
        symbol(10, "second", 1),
        symbol(20, "third", 1),
    ];
    let files = [FileEntry {
        id: FileId::new(1),
        path: PathBuf::from("a.cpp"),
        hash: ContentHash::compute(b"a"),
        size: 0,
        modified: 0,
    }];
    let file = GraphFile::build(symbols, &[], &files).unwrap();
    let names = |symbols: Vec<Symbol>| -> Vec<String> {
        symbols.into_iter().map(|symbol| symbol.name).collect()
    };
    assert_eq!(
        names(file.symbols_in_file(FileId::new(1))),
        ["first", "second", "third"]
    );
    // A graph built again from what it holds keeps it too.
    let again = GraphFile::build(file.all_symbols(), &[], &file.files()).unwrap();
    assert_eq!(
        names(again.symbols_in_file(FileId::new(1))),
        ["first", "second", "third"]
    );
}

#[test]
fn an_empty_graph_opens() {
    let file = GraphFile::build(Vec::new(), &[], &[]).unwrap();
    assert_eq!((file.symbol_count(), file.edge_count()), (0, 0));
}

#[test]
fn broken_bytes_are_refused_and_never_stop_it() {
    let good = sample();
    let (symbols, edges) = (good.symbol_bytes().to_vec(), good.edge_bytes().to_vec());
    assert!(GraphFile::open(Vec::new(), edges.clone()).is_err());
    assert!(GraphFile::open(symbols.clone(), Vec::new()).is_err());
    assert!(GraphFile::open(symbols[..symbols.len() / 2].to_vec(), edges.clone()).is_err());
    assert!(GraphFile::open(symbols.clone(), edges[..edges.len() / 2].to_vec()).is_err());
    let ask = |file: GraphFile| {
        file.all_symbols();
        file.all_edges();
        file.files();
        for id in [10, 20, 30, 40] {
            file.get_symbol(SymbolId::new(id));
            file.outgoing(SymbolId::new(id));
            file.incoming(SymbolId::new(id));
        }
        file.find_symbol("same");
        file.symbols_in_file(FileId::new(1));
    };
    // Any byte changed: refused, or still a graph that answers without a panic.
    for at in 0..symbols.len() {
        let mut changed = symbols.clone();
        changed[at] ^= 0xff;
        if let Ok(file) = GraphFile::open(changed, edges.clone()) {
            ask(file);
        }
    }
    for at in 0..edges.len() {
        let mut changed = edges.clone();
        changed[at] ^= 0xff;
        if let Ok(file) = GraphFile::open(symbols.clone(), changed) {
            ask(file);
        }
    }
}

#[test]
fn numbers_of_every_enum_go_both_ways() {
    for number in 0..=u8::MAX {
        if let Some(kind) = kind_of(number) {
            assert_eq!(kind_number(kind), number);
        }
        if let Some(language) = language_of(number) {
            assert_eq!(language_number(language), number);
        }
        if let Some(visibility) = visibility_of(number) {
            assert_eq!(visibility_number(visibility), number);
        }
        if let Some(kind) = edge_of(number) {
            assert_eq!(edge_number(kind), number);
        }
        if let Some(confidence) = confidence_of(number) {
            assert_eq!(confidence_number(confidence), number);
        }
    }
}
