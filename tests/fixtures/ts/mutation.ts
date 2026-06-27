// ventouse TS fixtures — Mutation with type precision (P1). `readonly`/`Readonly<T>` params can't
// be mutated by the callee (compiler-enforced, like C++ `const`) -> never input mutation.

interface Box {
  x: number;
}

// TS-MUT-ARR — a mutable array param mutated -> dirty 20
function pushIt(arr: number[], x: number): void {
  arr.push(x);
}

// TS-MUT-OBJ — a mutable object param field written -> dirty 20
function setX(o: Box, v: number): void {
  o.x = v;
}

// TS-MUT-READONLY-ARR — `readonly number[]` cannot be mutated -> clean 0
function total(arr: readonly number[]): number {
  return arr.reduce((a, b) => a + b, 0);
}

// TS-MUT-READONLY-OBJ — `Readonly<Box>` param cannot be mutated -> clean 0
function getX(o: Readonly<Box>): number {
  return o.x;
}

// TS-MUT-LOCAL — local owned array, mutated locally -> clean 0
function build(n: number): number[] {
  const a: number[] = [];
  for (let i = 0; i < n; i++) {
    a.push(i);
  }
  return a;
}
