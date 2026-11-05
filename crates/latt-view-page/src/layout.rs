//! Where each symbol starts: crates on a spiral, files inside a crate, symbols inside a file. It shows
//! the project's structure, and a symbol is then held there by a weak spring.

use std::collections::BTreeMap;

const GOLDEN_ANGLE: f32 = 2.399_963_2;

/// What a path groups under: `crates/a` for `crates/a/src/lib.rs`.
fn group_of(path: &str) -> &str {
    let mut parts = path.splitn(3, '/');
    let first = parts.next().unwrap_or(path);
    match (first, parts.next()) {
        ("crates" | "third_party" | "bindings" | "sdks" | "packages", Some(second)) => {
            &path[..first.len() + 1 + second.len()]
        }
        _ => first,
    }
}

fn on_spiral(place: usize, count: usize, radius: f32) -> [f32; 2] {
    let distance = radius * ((place as f32 + 0.5) / count.max(1) as f32).sqrt();
    let angle = place as f32 * GOLDEN_ANGLE;
    [distance * angle.cos(), distance * angle.sin()]
}

/// A place for each symbol, given its file's place in `files`.
pub fn seed(files: &[String], symbol_files: &[u32]) -> Vec<[f32; 2]> {
    let mut by_file: Vec<Vec<usize>> = vec![Vec::new(); files.len()];
    for (symbol, file) in symbol_files.iter().enumerate() {
        if let Some(symbols) = by_file.get_mut(*file as usize) {
            symbols.push(symbol);
        }
    }
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (file, path) in files.iter().enumerate() {
        groups.entry(group_of(path)).or_default().push(file);
    }
    let size = |group: &[usize]| group.iter().map(|file| by_file[*file].len()).sum::<usize>();
    let mut ordered: Vec<&Vec<usize>> = groups.values().collect();
    ordered.sort_by_key(|group| std::cmp::Reverse(size(group)));

    let total = symbol_files.len().max(1) as f32;
    let mut places = vec![[0.0, 0.0]; symbol_files.len()];
    for (place, group) in ordered.iter().enumerate() {
        let centre = on_spiral(place, ordered.len(), 0.85);
        let group_size = size(group).max(1) as f32;
        let radius = 0.55 * (group_size / total).sqrt();
        let mut in_order: Vec<usize> = (*group).clone();
        in_order.sort_by_key(|file| std::cmp::Reverse(by_file[*file].len()));
        for (file_place, file) in in_order.iter().enumerate() {
            let at = on_spiral(file_place, in_order.len(), radius);
            let inner = radius * 0.6 * (by_file[*file].len() as f32 / group_size).sqrt();
            for (symbol_place, symbol) in by_file[*file].iter().enumerate() {
                let near = on_spiral(symbol_place, by_file[*file].len(), inner);
                places[*symbol] = [centre[0] + at[0] + near[0], centre[1] + at[1] + near[1]];
            }
        }
    }
    places
}

/// A place near `anchor`, for a symbol that came after the others.
pub fn near(anchor: [f32; 2], place: usize) -> [f32; 2] {
    let at = on_spiral(place % 64, 64, 0.02);
    [anchor[0] + at[0], anchor[1] + at[1]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_group_is_the_crate_or_the_top_directory() {
        assert_eq!(group_of("crates/a/src/lib.rs"), "crates/a");
        assert_eq!(group_of("src/lib.rs"), "src");
        assert_eq!(group_of("lib.rs"), "lib.rs");
    }

    #[test]
    fn test_symbols_of_a_crate_lie_closer_than_those_of_two_crates() {
        let files = ["crates/a/one.rs", "crates/a/two.rs", "crates/b/one.rs"].map(str::to_string);
        let symbol_files: Vec<u32> = (0..300).map(|i| i % 3).collect();
        let places = seed(&files, &symbol_files);
        let centre_of = |file: u32| {
            let inside: Vec<_> = (0..300).filter(|i| symbol_files[*i] == file).collect();
            let sum = inside.iter().fold([0.0, 0.0], |sum, i| {
                [sum[0] + places[*i][0], sum[1] + places[*i][1]]
            });
            [sum[0] / inside.len() as f32, sum[1] / inside.len() as f32]
        };
        let apart = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
        let same_crate = apart(centre_of(0), centre_of(1));
        let other_crate = apart(centre_of(0), centre_of(2));
        assert!(same_crate < other_crate, "{same_crate} {other_crate}");
    }

    #[test]
    fn test_the_same_project_gets_the_same_places() {
        let files = ["a.rs", "b.rs"].map(str::to_string);
        assert_eq!(seed(&files, &[0, 1, 0]), seed(&files, &[0, 1, 0]));
    }

    #[test]
    fn test_a_file_with_no_symbols_or_an_unknown_one_breaks_nothing() {
        assert_eq!(seed(&[], &[]), Vec::<[f32; 2]>::new());
        let files = ["a.rs"].map(str::to_string);
        assert_eq!(seed(&files, &[7]).len(), 1);
    }
}
