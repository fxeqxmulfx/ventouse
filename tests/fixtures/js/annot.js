// ventouse JS fixtures — the single force-clean override (P3). Comment pragma: `// ventouse: pure`.

// JS-ANN-OK — override on an already-clean fn is a no-op -> 0.
// ventouse: pure
function addPure(a, b) { return a + b; }

// JS-ANN-OVERRIDE — `// ventouse: pure` OVERWRITES the inferred IO dirt -> forced clean 0.
// ventouse: pure
function notPure(x) { console.log(x); }
