// ventouse JS fixtures — `this` / class contagion (P1/P2).

// JS-CLS — `this.v=` clean in the constructor, dirty in a regular method.
// set 20 + class 10 = 30.
class Box {
  constructor(v) { this.v = v; } // ctor self-mut -> clean 0
  set(v) { this.v = v; }         // self-mut outside ctor -> dirty 20
  get() { return this.v; }       // read -> clean 0
}

// JS-CLS-STATIC — a static method has no `this`; IO -> dirty 20; class 10. total 30.
class Emitter {
  static emit(x) { console.log(x); }
}

// JS-CLS-NEW — `new` runs a dirty constructor -> the caller is infected (P2).
// ctor 20 + class 10 + startLogger 20 = 50.
class Logger {
  constructor(p) { console.log(p); }
}
function startLogger() { new Logger("f"); } // infected (owned 1) -> 20
