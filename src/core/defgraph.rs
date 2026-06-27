//! The definition graph — entities + their reference edges, built directly from a module's
//! `(entities, edges)` (which the scope graph already produced). The substrate for `placement`
//! (gap-to-deps) and `declorder` (declare-before-use). Per-module: locality is same-file only.

use std::collections::HashMap;

use crate::core::model::EntityKind;
use crate::core::raw::{RawEntity, RawKind};

pub struct Def {
    pub qualname: String,
    pub kind: EntityKind,
    pub line: u32,
    /// Indices (into `DefGraph::defs`) of the definitions this one references.
    pub calls: Vec<usize>,
}

pub struct DefGraph {
    pub file: String,
    pub defs: Vec<Def>,
}

impl DefGraph {
    /// Build the graph for one module: entities become nodes; `(caller, callee)` qualname edges
    /// become adjacency (edges to names not in this module are dropped).
    pub fn build(file: &str, entities: &[RawEntity], edges: &[(String, String)]) -> DefGraph {
        let mut index: HashMap<&str, usize> = HashMap::with_capacity(entities.len());
        let mut defs = Vec::with_capacity(entities.len());
        for (i, e) in entities.iter().enumerate() {
            index.insert(e.qualname.as_str(), i);
            defs.push(Def { qualname: e.qualname.clone(), kind: entity_kind(e.kind), line: e.line, calls: Vec::new() });
        }
        for (caller, callee) in edges {
            if let (Some(&c), Some(&t)) = (index.get(caller.as_str()), index.get(callee.as_str())) {
                defs[c].calls.push(t);
            }
        }
        DefGraph { file: file.to_string(), defs }
    }
}

fn entity_kind(k: RawKind) -> EntityKind {
    match k {
        RawKind::Module => EntityKind::Module,
        RawKind::Function => EntityKind::Function,
        RawKind::Method => EntityKind::Method,
        RawKind::Class => EntityKind::Class,
        RawKind::Data => EntityKind::Binding, // a data definition (constant / attribute)
    }
}
