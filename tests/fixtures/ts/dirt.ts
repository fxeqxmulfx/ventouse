// ventouse TS fixtures — Dirt (P1 effects, P2 contagion). dirty = 10 + 10*owned_lines.
// Same core as JS; TS only enriches the surface (types -> precision). See typed_precision.ts.

// TS-A — pure arithmetic (typed) -> clean 0
function add(a: number, b: number): number {
  return a + b;
}

// TS-B — IO (console.log) -> dirty 20 (owned 1)
function logIt(x: number): void {
  console.log(x);
}

// TS-D — contagion through a call (P2): wlog dirty 20, run infected (owned 2) -> 30. total 50
function wlog(m: number): void {
  console.log(m);
}
function run(d: number): void {
  const r = d * 2;
  wlog(r);
}
