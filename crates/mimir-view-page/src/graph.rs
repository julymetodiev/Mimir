//! The graph as the page knows it: finding, relating, and keeping places across snapshots.

use crate::layout;
use mimir_view_wire::{EDGE_CALL, EDGE_POSSIBLE, EDGE_USE, Snapshot};
use std::collections::{HashMap, VecDeque};

/// A snapshot, with what the page asks of it made quick.
pub struct Model {
    pub snapshot: Snapshot,
    by_id: HashMap<u64, u32>,
    lower_names: Vec<String>,
}

/// The symbols related to one, by how.
#[derive(Debug, Default, PartialEq)]
pub struct Related {
    pub called_by: Vec<u32>,
    pub calls: Vec<u32>,
    pub possibly: Vec<u32>,
    pub used_by: Vec<u32>,
    pub uses: Vec<u32>,
}

impl Related {
    /// All of them, for lighting up.
    pub fn all(&self) -> impl Iterator<Item = u32> + '_ {
        self.called_by
            .iter()
            .chain(&self.calls)
            .chain(&self.possibly)
            .chain(&self.used_by)
            .chain(&self.uses)
            .copied()
    }
}

/// Where each symbol is and where it is held.
pub struct Places {
    pub at: Vec<[f32; 2]>,
    pub home: Vec<[f32; 2]>,
}

impl Model {
    pub fn new(snapshot: Snapshot) -> Self {
        let by_id = snapshot
            .ids
            .iter()
            .enumerate()
            .map(|(place, id)| (*id, place as u32))
            .collect();
        let lower_names = snapshot.names.iter().map(|n| n.to_lowercase()).collect();
        Self {
            snapshot,
            by_id,
            lower_names,
        }
    }

    pub fn place_of(&self, id: u64) -> Option<u32> {
        self.by_id.get(&id).copied()
    }

    pub fn file_of(&self, symbol: u32) -> &str {
        self.snapshot
            .symbol_files
            .get(symbol as usize)
            .and_then(|file| self.snapshot.files.get(*file as usize))
            .map_or("", String::as_str)
    }

    pub fn name_of(&self, symbol: u32) -> &str {
        self.snapshot
            .names
            .get(symbol as usize)
            .map_or("", String::as_str)
    }

    pub fn related(&self, symbol: u32) -> Related {
        let mut related = Related::default();
        for [from, to, kind] in &self.snapshot.edges {
            match *kind {
                EDGE_CALL if *from == symbol => related.calls.push(*to),
                EDGE_CALL if *to == symbol => related.called_by.push(*from),
                EDGE_POSSIBLE if *from == symbol => related.possibly.push(*to),
                EDGE_POSSIBLE if *to == symbol => related.possibly.push(*from),
                EDGE_USE if *from == symbol => related.uses.push(*to),
                EDGE_USE if *to == symbol => related.used_by.push(*from),
                _ => {}
            }
        }
        for list in [
            &mut related.called_by,
            &mut related.calls,
            &mut related.possibly,
            &mut related.used_by,
            &mut related.uses,
        ] {
            list.sort_by(|a, b| self.name_of(*a).cmp(self.name_of(*b)).then(a.cmp(b)));
            list.dedup();
        }
        related
    }

    /// Symbols with this text in their name: those that start with it first, then the shortest.
    pub fn search(&self, text: &str, most: usize) -> Vec<u32> {
        let text = text.to_lowercase();
        if text.is_empty() {
            return Vec::new();
        }
        let mut found: Vec<u32> = (0..self.lower_names.len() as u32)
            .filter(|symbol| self.lower_names[*symbol as usize].contains(&text))
            .collect();
        found.sort_by_key(|symbol| {
            let name = &self.lower_names[*symbol as usize];
            (!name.starts_with(&text), name.len(), *symbol)
        });
        found.truncate(most);
        found
    }

    /// The symbol of a new snapshot that an old one's symbol became, by file and name.
    pub fn same_symbol(&self, old: &Model, symbol: u32) -> Option<u32> {
        let (file, name) = (old.file_of(symbol), old.name_of(symbol));
        (0..self.snapshot.names.len() as u32)
            .find(|candidate| self.name_of(*candidate) == name && self.file_of(*candidate) == file)
    }

    /// Places for a first snapshot.
    pub fn first_places(&self) -> Places {
        let home = layout::seed(&self.snapshot.files, &self.snapshot.symbol_files);
        Places {
            at: home.clone(),
            home,
        }
    }

    /// Places for this snapshot after `old`: a symbol keeps its place by file and name, and a new
    /// one goes near its file. The picture does not jump because the project changed.
    pub fn places_after(&self, old: &Model, old_places: &Places) -> Places {
        let mut kept: HashMap<(&str, &str), VecDeque<usize>> = HashMap::new();
        let mut anchors: HashMap<&str, [f32; 2]> = HashMap::new();
        for symbol in 0..old.snapshot.names.len() {
            let file = old.file_of(symbol as u32);
            kept.entry((file, old.name_of(symbol as u32)))
                .or_default()
                .push_back(symbol);
            if let Some(home) = old_places.home.get(symbol) {
                anchors.entry(file).or_insert(*home);
            }
        }
        let seeded = layout::seed(&self.snapshot.files, &self.snapshot.symbol_files);
        let mut places = Places {
            at: Vec::with_capacity(seeded.len()),
            home: Vec::with_capacity(seeded.len()),
        };
        for (symbol, seed) in seeded.iter().enumerate() {
            let file = self.file_of(symbol as u32);
            let earlier = kept
                .get_mut(&(file, self.name_of(symbol as u32)))
                .and_then(VecDeque::pop_front);
            match earlier.and_then(|e| Some((*old_places.at.get(e)?, *old_places.home.get(e)?))) {
                Some((at, home)) => {
                    places.at.push(at);
                    places.home.push(home);
                }
                None => {
                    let home = anchors
                        .get(file)
                        .map_or(*seed, |anchor| layout::near(*anchor, symbol));
                    places.at.push(home);
                    places.home.push(home);
                }
            }
        }
        places
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(names: &[(&str, u32)], edges: Vec<[u32; 3]>) -> Model {
        Model::new(Snapshot {
            files: vec!["a.rs".to_string(), "b.rs".to_string()],
            ids: (0..names.len() as u64).map(|i| i + 100).collect(),
            names: names.iter().map(|(n, _)| (*n).to_string()).collect(),
            kinds: vec![0; names.len()],
            symbol_files: names.iter().map(|(_, f)| *f).collect(),
            lines: vec![1; names.len()],
            edges,
            ..Snapshot::default()
        })
    }

    #[test]
    fn test_related_says_how_and_lists_each_once() {
        let m = model(
            &[("main", 0), ("run", 0), ("Config", 1), ("helper", 1)],
            vec![
                [0, 1, EDGE_CALL],
                [0, 1, EDGE_CALL],
                [1, 3, EDGE_CALL],
                [1, 2, EDGE_USE],
                [1, 3, EDGE_POSSIBLE],
            ],
        );
        let related = m.related(1);
        assert_eq!(related.called_by, [0]);
        assert_eq!(related.calls, [3]);
        assert_eq!(related.uses, [2]);
        assert_eq!(related.possibly, [3]);
        assert_eq!(m.related(2).used_by, [1]);
    }

    #[test]
    fn test_search_finds_any_case_with_those_that_start_with_it_first() {
        let m = model(
            &[
                ("unrelated", 0),
                ("load_all", 0),
                ("Load", 1),
                ("reload", 1),
            ],
            vec![],
        );
        assert_eq!(m.search("load", 10), [2, 1, 3]);
        assert_eq!(m.search("load", 1), [2]);
        assert_eq!(m.search("", 10), Vec::<u32>::new());
    }

    #[test]
    fn test_a_symbol_keeps_its_place_and_a_new_one_goes_near_its_file() {
        let old = model(&[("kept", 0), ("gone", 1)], vec![]);
        let old_places = Places {
            at: vec![[0.5, 0.5], [0.9, 0.9]],
            home: vec![[0.4, 0.4], [0.8, 0.8]],
        };
        let new = model(&[("new", 0), ("kept", 0)], vec![]);
        let places = new.places_after(&old, &old_places);
        assert_eq!(places.at[1], [0.5, 0.5]);
        assert_eq!(places.home[1], [0.4, 0.4]);
        let apart = (places.home[0][0] - 0.4).hypot(places.home[0][1] - 0.4);
        assert!(apart < 0.05, "{apart}");
    }

    #[test]
    fn test_ids_change_but_the_same_symbol_is_found_by_file_and_name() {
        let old = model(&[("a", 0), ("b", 1)], vec![]);
        let new = model(&[("b", 1), ("a", 0)], vec![]);
        assert_eq!(new.same_symbol(&old, 0), Some(1));
        assert_eq!(new.place_of(100), Some(0));
        assert_eq!(new.place_of(7), None);
    }
}
