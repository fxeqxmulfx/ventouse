// ventouse JS fixtures — Mutation forms (P1). Surface: primitives are copied (rebind != mutation).

// JS-MUT1 — subscript assign on a param -> dirty 20
function setItem(o, k, v) { o[k] = v; }

// JS-MUT2 — property assign on a param -> dirty 20
function setX(o, v) { o.x = v; }

// JS-MUT3 — delete on a param -> dirty 20
function rm(o, k) { delete o[k]; }

// JS-MUT4 — Object.assign(param, ...) mutates the target param -> dirty 20
function merge(target, src) { Object.assign(target, src); }

// JS-MUT5 — primitive param `+=` is a REBIND (primitive copied), not mutation -> clean 0
function addOne(x) { x += 1; return x; }

// JS-MUT6 — local owned array, mutated locally -> clean 0
function build(n) {
  const a = [];
  for (let i = 0; i < n; i++) { a.push(i); }
  return a;
}
