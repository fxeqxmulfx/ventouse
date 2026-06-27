// JavaScript / TypeScript — see DESIGN per-language examples
function pushIt(arr, x) { arr.push(x); }            // input mutation -> dirty 20
function logIt(x) { console.log(x); }               // IO -> dirty 20
function addOne(x) { x += 1; return x; }            // primitive rebind -> clean 0
class Box { constructor(v){ this.v = v; }            // ctor self-mut -> clean
            set(v){ this.v = v; } }                  // set: self-mut outside ctor -> dirty 20 (class 10)
function build(n){ const a=[]; for(let i=0;i<n;i++) a.push(i); return a; }  // local owned -> clean 0
