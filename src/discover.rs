//! File discovery: collect analyzable source files under a path, skipping vendored/cache dirs.

use std::path::Path;

use walkdir::WalkDir;

const SKIP_DIRS: &[&str] = &[
    "venv", ".venv", "__pycache__", "site-packages", "node_modules", ".git", "target", ".mypy_cache",
];

fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension().and_then(|e| e.to_str()) == Some(ext)
}

fn is_skipped_dir(p: &Path) -> bool {
    p.is_dir()
        && p.file_name()
            .and_then(|n| n.to_str())
            .map(|n| SKIP_DIRS.contains(&n))
            .unwrap_or(false)
}

/// Collect files with extension `ext` under `root` (a file or directory), excluding vendored/cache
/// directories. Returns sorted paths as strings.
pub fn source_files(root: &str, ext: &str) -> Vec<String> {
    let p = Path::new(root);
    if p.is_file() {
        return if has_ext(p, ext) {
            vec![root.to_string()]
        } else {
            Vec::new()
        };
    }
    let mut files: Vec<String> = WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !is_skipped_dir(e.path()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && has_ext(e.path(), ext))
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();
    files.sort();
    files
}

/// Collect `.py` files (see [`source_files`]).
pub fn python_files(root: &str) -> Vec<String> {
    source_files(root, "py")
}

/// C++ source/header extensions (a translation unit or an in-header definition can carry any of them).
pub const CPP_EXTS: &[&str] = &["cpp", "cc", "cxx", "hpp", "hh", "hxx", "h"];

/// Collect C++ files across all [`CPP_EXTS`], de-duplicated and sorted (see [`source_files`]).
pub fn cpp_files(root: &str) -> Vec<String> {
    let mut files: Vec<String> = CPP_EXTS.iter().flat_map(|ext| source_files(root, ext)).collect();
    files.sort();
    files.dedup();
    files
}

/// TypeScript / JavaScript extensions (`.tsx`/`.jsx` carry JSX; `.ts`/`.js` plain).
pub const TS_EXTS: &[&str] = &["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"];

/// Collect TS/JS files across all [`TS_EXTS`], de-duplicated and sorted (see [`source_files`]).
/// Declaration files (`.d.ts`) are types-only — excluded.
pub fn ts_files(root: &str) -> Vec<String> {
    let mut files: Vec<String> = TS_EXTS
        .iter()
        .flat_map(|ext| source_files(root, ext))
        .filter(|f| !f.ends_with(".d.ts"))
        .collect();
    files.sort();
    files.dedup();
    files
}
