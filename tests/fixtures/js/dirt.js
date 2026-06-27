// ventouse JS fixtures — Dirt (P1 effects, P2 contagion). dirty = 10 + 10*owned_lines.

// JS-A — pure arithmetic -> clean 0
function add(a, b) { return a + b; }

// JS-B — IO (console.log) -> dirty 20 (owned 1)
function logIt(x) { console.log(x); }

// JS-C — input mutation (array mutator on a param) -> dirty 20 (owned 1)
function pushIt(arr, x) { arr.push(x); }

// JS-GR — module-global READ -> clean 0 ; JS-GW — module-global WRITE -> dirty 20
let COUNT = 0;
function readCount() { return COUNT + 1; } // clean 0
function bump() { COUNT += 1; }            // module-global rebind -> dirty 20 (owned 1)

// JS-D — contagion through a call (P2): wlog dirty 20, run infected (owned 2) -> 30. total 50
function wlog(m) { console.log(m); }
function run(d) {
  const r = d * 2;
  wlog(r);
}
