// ventouse TS fixtures — TS-TYPE-ONLY: type-only constructs are erased at runtime: NOT entities,
// no findings. Only the concrete `impl` of `f` is an entity (pure -> 0). Whole file -> 0.

interface Shape {
  area(): number; // a type member, not a function entity
}

type Id = string | number;

declare function ext(x: number): number; // ambient decl -> no body, not an entity

abstract class Base {
  abstract run(): void; // abstract -> no body, not an entity
}

function f(x: number): number; // overload signature -> no body, skipped
function f(x: number): number {
  return x + 1; // the impl is the only entity -> clean 0
}
