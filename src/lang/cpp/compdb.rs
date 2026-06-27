//! `compile_commands.json` (a *compilation database*) support: the precise `-I`/`-D`/`-std` flags a
//! real build uses, so libclang resolves types and stops degrading the AST (the root cause behind the
//! frontend's residual false positives). When no database is found, the caller falls back to the
//! heuristic include-dir guess (`super::include_dirs`).
//!
//! Only the flags that affect TYPE RESOLUTION are taken (`-I`/`-isystem`/`-iquote` → emitted as `-I`,
//! `-D`, `-std`); the rest of a compile command is driver noise (`-c`, `-o out.o`, `-W…`, the source
//! file) that would confuse libclang's parse API. Relative include paths resolve against the entry's
//! `directory`. Flags are aggregated across ALL entries (deduped): headers aren't their own entries,
//! so a per-file lookup would miss them — the project's union of include dirs applies to every file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Find `compile_commands.json` at or above `start` (also checking a `build/` subdir at each level).
fn locate(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    for _ in 0..32 {
        let d = dir?;
        for cand in [d.join("compile_commands.json"), d.join("build/compile_commands.json")] {
            if cand.is_file() {
                return Some(cand);
            }
        }
        dir = d.parent();
    }
    None
}

/// Split a shell `command` string into tokens (respecting `"`/`'` quotes and `\` escapes) — used when
/// an entry gives a `command` string rather than an `arguments` array.
fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

/// Emit the resolution-affecting flags from one entry's tokens as self-contained args (`-I<abs>`,
/// `-D<def>`, `-std=…`). `-isystem`/`-iquote` are folded to `-I` (the distinction doesn't matter for
/// our use). Relative include dirs resolve against `directory`.
fn extract(args: &[String], directory: &Path, out: &mut Vec<String>) {
    let absdir = |p: &str| {
        let pp = Path::new(p);
        if pp.is_absolute() { p.to_string() } else { directory.join(pp).to_string_lossy().into_owned() }
    };
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if let Some(p) = a.strip_prefix("-I").filter(|s| !s.is_empty()) {
            out.push(format!("-I{}", absdir(p)));
        } else if let Some(p) = a.strip_prefix("-isystem").filter(|s| !s.is_empty()) {
            out.push(format!("-I{}", absdir(p)));
        } else if let Some(d) = a.strip_prefix("-D").filter(|s| !s.is_empty()) {
            out.push(format!("-D{d}"));
        } else if a.starts_with("-std=") {
            out.push(a.to_string());
        } else if matches!(a, "-I" | "-isystem" | "-iquote")
            && let Some(n) = args.get(i + 1)
        {
            out.push(format!("-I{}", absdir(n)));
            i += 1;
        } else if a == "-D"
            && let Some(n) = args.get(i + 1)
        {
            out.push(format!("-D{n}"));
            i += 1;
        }
        i += 1;
    }
}

/// The compile flags for `files` from a `compile_commands.json` near their root, or `None` if no
/// database is found (caller then uses the heuristic). Aggregated + deduped across all entries.
pub(super) fn flags(files: &[(&str, &str)]) -> Option<Vec<String>> {
    let root = super::common_ancestor(files)?;
    let db = locate(Path::new(&root))?;
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(db).ok()?).ok()?;

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for e in json.as_array()? {
        let directory = e.get("directory").and_then(|v| v.as_str()).map(PathBuf::from).unwrap_or_default();
        let args: Vec<String> = if let Some(arr) = e.get("arguments").and_then(|v| v.as_array()) {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else if let Some(cmd) = e.get("command").and_then(|v| v.as_str()) {
            shell_split(cmd)
        } else {
            continue;
        };
        let mut entry_flags = Vec::new();
        extract(&args, &directory, &mut entry_flags);
        for f in entry_flags {
            if seen.insert(f.clone()) {
                out.push(f);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_split_handles_quotes_and_escapes() {
        // an adjacent quote concatenates into one token (shell semantics): `-D"X=1 2"` → `-DX=1 2`.
        assert_eq!(shell_split(r#"c++ -I/a b -D"X=1 2""#), ["c++", "-I/a", "b", "-DX=1 2"]);
        assert_eq!(shell_split(r"-I/with\ space x"), ["-I/with space", "x"]);
    }

    #[test]
    fn extract_takes_resolution_flags_and_resolves_relative_dirs() {
        let dir = Path::new("/proj/build");
        let args: Vec<String> = ["c++", "-c", "-I/abs", "-Irel", "-isystem", "/sys", "-DA=1", "-D", "B",
            "-std=c++20", "-o", "out.o", "-Wall", "main.cpp"]
            .iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        extract(&args, dir, &mut out);
        // -I joined, -I relative (→ resolved against dir), -isystem (→ -I), -D joined, -D separated, -std;
        // driver noise (-c, -o out.o, -Wall, main.cpp) dropped.
        assert_eq!(out, ["-I/abs", "-I/proj/build/rel", "-I/sys", "-DA=1", "-DB", "-std=c++20"]);
    }
}
