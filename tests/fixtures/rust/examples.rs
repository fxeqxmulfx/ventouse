// Rust — see DESIGN per-language examples
fn push_it(v: &mut Vec<i32>, x: i32) { v.push(x); }  // &mut param -> input mutation -> dirty
fn add(a: i32, b: i32) -> i32 { a + b }              // arithmetic -> clean
const fn double(x: i32) -> i32 { x * 2 }             // const fn auto-signal -> clean
struct S { n: i32 }
impl S {
    fn new(n: i32) -> Self { S { n } }               // constructor (builds value) -> clean
    fn set(&mut self, n: i32) { self.n = n; }        // &mut self setter -> dirty
    fn get(&self) -> i32 { self.n }                  // &self -> cannot mutate -> clean
}
fn log_it(x: i32) { println!("{x}"); }               // IO -> dirty
