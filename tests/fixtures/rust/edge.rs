// ventouse Rust fixtures — edges (P1/P2 surface). Rust has no exceptions: errors are values.

// RS-EC-RESULT — error handling via Result is pure VALUES (not an effect) -> clean 0.
fn checked(x: i32) -> Result<i32, &'static str> {
    if x < 0 {
        return Err("neg");
    }
    Ok(x)
}

// RS-EC-CYCLE — mutual recursion resolved by a fixpoint (P2): both dirty.
fn ping(n: i32) { println!("{n}"); pong(n); } // IO -> dirty
fn pong(n: i32) { ping(n); }                   // infected via ping
