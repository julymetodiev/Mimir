use super::*;

#[test]
fn a_python_module_finds_its_file() {
    let (a, b, c, d, e) = (
        FileId::new(1),
        FileId::new(2),
        FileId::new(3),
        FileId::new(4),
        FileId::new(5),
    );
    let by_path: HashMap<&Path, FileId> = [
        (Path::new("pkg/a.py"), a),
        (Path::new("pkg/__init__.py"), b),
        (Path::new("pkg/sub/c.pyi"), c),
        (Path::new("top.py"), d),
        (Path::new("src/lib/__init__.py"), e),
    ]
    .into_iter()
    .collect();
    let from = Path::new("pkg/sub/x.py");
    assert_eq!(resolve_import(from, ".c", &by_path), Some(c));
    assert_eq!(resolve_import(from, "..a", &by_path), Some(a));
    assert_eq!(resolve_import(from, "..", &by_path), Some(b));
    assert_eq!(resolve_import(from, "pkg.a", &by_path), Some(a));
    assert_eq!(resolve_import(from, "pkg", &by_path), Some(b));
    assert_eq!(resolve_import(from, "top", &by_path), Some(d));
    assert_eq!(resolve_import(from, "lib", &by_path), Some(e));
    assert_eq!(resolve_import(from, "missing", &by_path), None);
    assert_eq!(resolve_import(from, ".", &by_path), None);
    assert_eq!(resolve_import(from, "....far", &by_path), None);
}

#[test]
fn a_relative_import_finds_its_file() {
    let (a, b, c, d) = (
        FileId::new(1),
        FileId::new(2),
        FileId::new(3),
        FileId::new(4),
    );
    let by_path: HashMap<&Path, FileId> = [
        (Path::new("src/a.ts"), a),
        (Path::new("src/lib/index.ts"), b),
        (Path::new("src/c.tsx"), c),
        (Path::new("d.js"), d),
    ]
    .into_iter()
    .collect();
    let from = Path::new("src/x.ts");
    assert_eq!(resolve_import(from, "./a", &by_path), Some(a));
    assert_eq!(resolve_import(from, "./a.js", &by_path), Some(a));
    assert_eq!(resolve_import(from, "./lib", &by_path), Some(b));
    assert_eq!(resolve_import(from, "./c", &by_path), Some(c));
    assert_eq!(resolve_import(from, "../d", &by_path), Some(d));
    assert_eq!(resolve_import(from, "../d.js", &by_path), Some(d));
    assert_eq!(resolve_import(from, "./missing", &by_path), None);
    assert_eq!(resolve_import(from, "../../out", &by_path), None);
}
