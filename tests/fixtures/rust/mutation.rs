// ventouse Rust fixtures — Mutation (P1). Signature-precise: only `&mut` can mutate the caller.

// RS-MUT-REF — write through a &mut param -> dirty 20
fn set_ref(p: &mut i32) { *p = 5; }

// RS-MUT-SHARED — a &T param: the compiler forbids mutation -> never input mutation -> clean 0
fn read_ref(p: &i32) -> i32 { *p }

// RS-MUT-BYVAL — `mut v` BY VALUE is owned (moved in); mutating it is local -> clean 0
fn consume(mut v: Vec<i32>) -> usize { v.push(1); v.len() }

// RS-MUT-LOCAL — a local owned vec, mutated locally -> clean 0
fn build(n: i32) -> Vec<i32> {
    let mut out = Vec::new();
    for i in 0..n { out.push(i); }
    out
}
