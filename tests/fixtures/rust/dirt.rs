// ventouse Rust fixtures — Dirt (P1 effects, P2 contagion). dirty = 10 + 10*owned_lines.

// RS-A — pure arithmetic -> clean 0
fn add(a: i32, b: i32) -> i32 { a + b }

// RS-B — IO (println!) -> dirty 20 (owned 1)
fn log_it(x: i32) { println!("{x}"); }

// RS-C — input mutation through a &mut param -> dirty 20 (owned 1)
fn push_it(v: &mut Vec<i32>, x: i32) { v.push(x); }

// RS-C-CLEAN — &T cannot mutate the caller (signature-precise) -> clean 0
fn sum_it(v: &Vec<i32>) -> i32 { v.iter().sum() }

// RS-GLOBAL — `static mut` write -> dirty ; `const` read -> clean.
const MAX: i32 = 10;
static mut COUNTER: i32 = 0;
fn read_max() -> i32 { MAX }           // const read -> clean 0
fn bump() { unsafe { COUNTER += 1; } } // static mut write -> dirty 20 (owned 1)

// RS-D — contagion (P2): wlog dirty 20, run infected (owned 2) -> 30. total 50.
fn wlog(m: i32) { println!("{m}"); }
fn run(d: i32) {
    let r = d * 2;
    wlog(r);
}
