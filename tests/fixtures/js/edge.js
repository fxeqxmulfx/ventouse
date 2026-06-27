// ventouse JS fixtures — edges (P1/P2 surface).

// JS-EC-THROW — `throw` is control flow, NOT an effect (P1) -> clean 0.
function checked(x) {
  if (x < 0) { throw new Error("neg"); }
  return x;
}

// JS-EC-LAZY — a generator created but not iterated does not run its body (P2).
// gen dirty (owned 2) -> 30 ; make only creates it -> clean 0.
function* gen() {
  console.log("x");
  yield 1;
}
function make() {
  const g = gen();
  return g;
}

// JS-EC-GLOBALTHIS — writing a global object -> global mutation -> dirty 20.
function leak() { globalThis.cache = {}; }

// JS-EC-VAR — `var` is function-scoped/hoisted, so `levels` can't narrow it to a real inner block
// (like Python); `let`/`const` are block-scoped, so `levels` narrows their real runtime scope.
function varScope(a) {
  var x = a + 1;
  return x;
}
