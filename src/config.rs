//! Configuration from `pyproject.toml` (`[tool.ventouse.weights]`). Minimal hand-rolled reader —
//! no TOML dependency yet. Overrides the arbitrary default scoring constant (P4).

use std::path::{Path, PathBuf};

use crate::core::{DeclOrder, Weights};

/// Apply one `key = value` config entry to `w` (unknown keys are ignored). Every tunable in
/// [`Weights`] is settable; numeric keys parse as `u32`/`usize`, `order` as a string.
fn apply(w: &mut Weights, key: &str, val: &str) {
    let n32 = || val.parse::<u32>().ok();
    let nsz = || val.parse::<usize>().ok();
    match key {
        "scope_level" => w.scope_level = n32().unwrap_or(w.scope_level),
        "shared_callers" => w.shared_callers = nsz().unwrap_or(w.shared_callers),
        "shared_uses" => w.shared_uses = n32().unwrap_or(w.shared_uses),
        "crowded_scope" => w.crowded_scope = n32().unwrap_or(w.crowded_scope),
        "reorder_wedges" => w.reorder_wedges = n32().unwrap_or(w.reorder_wedges),
        "narrow_levels" => w.narrow_levels = n32().unwrap_or(w.narrow_levels),
        "accessor_fanin" => w.accessor_fanin = nsz().unwrap_or(w.accessor_fanin),
        "order" => match val.trim_matches(['"', '\'']) {
            "bottom-up" | "bottom_up" => w.order = DeclOrder::BottomUp,
            "top-down" | "top_down" => w.order = DeclOrder::TopDown,
            _ => {}
        },
        _ => {}
    }
}

/// Parse `[tool.ventouse.weights]` keys from TOML text; unknown keys keep their default.
pub fn parse_weights(toml: &str) -> Weights {
    let mut w = Weights::default();
    let mut in_section = false;
    for line in toml.lines() {
        let l = line.trim();
        if let Some(header) = l.strip_prefix('[') {
            in_section = header.trim_end_matches(']').trim() == "tool.ventouse.weights";
            continue;
        }
        if !in_section || l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            apply(&mut w, k.trim(), v.trim().trim_end_matches(',').trim());
        }
    }
    w
}

fn find_pyproject(root: &str) -> Option<PathBuf> {
    let p = Path::new(root);
    let dir = if p.is_file() { p.parent()? } else { p };
    let cand = dir.join("pyproject.toml");
    cand.exists().then_some(cand)
}

/// Load weights from a `pyproject.toml` next to `root` (a file's dir, or the dir itself).
/// Falls back to defaults if absent/unreadable.
pub fn weights_from_pyproject(root: &str) -> Weights {
    match find_pyproject(root).and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => parse_weights(&text),
        None => Weights::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tunable_is_settable() {
        let toml = "\
[tool.ventouse.weights]
scope_level = 5
shared_callers = 6
shared_uses = 11
crowded_scope = 50
reorder_wedges = 2
narrow_levels = 3
accessor_fanin = 7
order = \"top-down\"
";
        let w = parse_weights(toml);
        assert_eq!(
            (w.scope_level, w.shared_callers, w.shared_uses, w.crowded_scope, w.reorder_wedges),
            (5, 6, 11, 50, 2)
        );
        assert_eq!((w.narrow_levels, w.accessor_fanin), (3, 7));
        assert_eq!(w.order, DeclOrder::TopDown);
    }

    #[test]
    fn unknown_keys_and_missing_section_keep_defaults() {
        // an unknown key is ignored; keys outside the section are ignored.
        let w = parse_weights("[other]\naccessor_fanin = 99\n[tool.ventouse.weights]\nnope = 1\n");
        assert_eq!(w, Weights::default());
    }
}
