// ventouse TS fixtures — edges (P1/P2 surface).

// TS-EC-THROW — `throw` is control flow, NOT an effect (P1) -> clean 0.
function checked(x: number): number {
  if (x < 0) {
    throw new Error("neg");
  }
  return x;
}

// TS-EC-ENUM — an `enum` is a runtime object declaration with no effect -> module clean.
enum Color {
  Red,
  Green,
}

// TS-EC-ASCONST — `as const` is an immutability hint; a module-level literal has no effect -> clean.
const CONFIG = { retries: 3 } as const;

// TS-EC-DECORATOR — a decorator is ignored for purity (the decorator call is not counted).
function sealed(_target: unknown): void {}

@sealed
class Service {
  handle(): number {
    return 1; // pure -> clean 0
  }
}
