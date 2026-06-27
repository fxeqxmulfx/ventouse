// ventouse TS fixtures — class surface (P1/P2). Parameter properties = ctor field init; access
// modifiers don't affect purity; `readonly` fields are an immutability auto-signal.

// TS-PARAMPROP — `constructor(private x)` is field initialization in the ctor -> clean.
// set 20 + class 10 = 30 (the setter is the only dirty entity).
class Box {
  constructor(private x: number) {} // ctor param property -> clean 0
  set(v: number): void {
    this.x = v; // self-mut outside ctor -> dirty 20
  }
  get(): number {
    return this.x; // read -> clean 0
  }
}

// TS-READONLY-FIELD — `readonly` field can't be reassigned after construction (auto-signal).
// A method that only reads it is clean.
class Point {
  readonly x = 1;
  show(): number {
    return this.x; // read -> clean 0
  }
}
