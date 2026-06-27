// ventouse TS fixtures — the single force-clean override (P3). Comment pragma: `// ventouse: pure`.

// TS-ANN-OK — override on an already-clean fn is a no-op -> 0.
// ventouse: pure
function addPure(a: number, b: number): number {
  return a + b;
}

// TS-ANN-OVERRIDE — `// ventouse: pure` OVERWRITES the inferred IO dirt -> forced clean 0.
// ventouse: pure
function notPure(x: number): void {
  console.log(x);
}
