// ventouse TS fixtures — types make receivers KNOWN, so operators/methods resolve precisely (P3),
// instead of the untyped-JS optimistic default. TS is signature-precise where annotated.

// TS-TYPED-PURE — `s: string` -> `s.toUpperCase()` resolves to a known-pure string method -> clean 0
function shout(s: string): string {
  return s.toUpperCase();
}

// TS-TYPED-DIRTY — a typed receiver also resolves an EFFECTFUL member precisely.
// `d: Date` -> `Date.now`-style/clock access is effectful (nondeterminism) -> dirty 20.
function stamp(d: Date): number {
  return d.getTime() + Date.now();
}

// TS-UNTYPED-OPTIMISTIC — no type (`any`) -> attribute/method on unknown stays OPTIMISTIC pure
// by default -> clean 0 (a bare call is still dirty; `--strict` flips reads to dirty).
function shout2(s: any): string {
  return s.toUpperCase();
}
