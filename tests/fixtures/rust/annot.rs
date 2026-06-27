// ventouse Rust fixtures — auto-signals & pragma (P3).

// RS-AUTO-CONSTFN — `const fn` is a compiler-guaranteed near-pure signal -> clean 0.
const fn double(x: i32) -> i32 { x * 2 }

// RS-AUTO-SELF — `&self` proves no self-mutation (auto-signal); IO/global writes are still dirty.
// (see `get(&self)` in self_class.rs)

// RS-ANN-OK — override on an already-clean fn is a no-op -> 0.
// ventouse: pure
fn add_pure(a: i32, b: i32) -> i32 { a + b }

// RS-ANN-OVERRIDE — `// ventouse: pure` OVERWRITES the inferred dirt (unresolved call) -> forced 0.
// ventouse: pure
fn trusted(x: i32) { external_io(x); }
