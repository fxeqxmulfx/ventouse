// ventouse Rust fixtures — callees-before-callers (P5). Items are order-free for the Rust compiler,
// but order matters for human top-down reading -> the warning is still emitted. Cycles are exempt.

// RS-W-BAD — caller declared above callee -> 1 forward-reference warning.
fn caller() -> i32 { callee() } // forward ref -> warning
fn callee() -> i32 { 1 }

// RS-W-GOOD — callee above caller -> 0 warnings.
fn leaf() -> i32 { 1 }
fn top() -> i32 { leaf() }
