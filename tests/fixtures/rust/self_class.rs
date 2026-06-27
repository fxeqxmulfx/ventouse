// ventouse Rust fixtures — struct + impl as the class-like unit (P1/P2).
// Rust has no language constructor: `fn new() -> Self` (building a value) is the "constructor".

struct S {
    n: i32,
}

impl S {
    // RS-NEW — constructor builds/returns a value -> clean 0
    fn new(n: i32) -> Self { S { n } }
    // RS-SET — a &mut self setter writes a field -> dirty 20
    fn set(&mut self, n: i32) { self.n = n; }
    // RS-GET — &self cannot mutate -> clean 0
    fn get(&self) -> i32 { self.n }
}
// S has a dirty method (set) -> the class-like unit is dirty: base 10 + set 20 = 30.
