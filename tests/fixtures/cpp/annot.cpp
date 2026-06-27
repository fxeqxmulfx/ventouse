// ventouse C++ fixtures — auto-signals & pragma (P3).
void external_io(int); // an external, unverifiable function (resolved at link time)

// CPP-AUTO-CONSTEXPR — `constexpr` is a purity signal -> clean 0.
constexpr int dbl(int x) { return x * 2; }

// CPP-AUTO-GNU — `[[gnu::const]]` / `[[gnu::pure]]` attribute -> purity hint -> clean 0.
[[gnu::const]] int square(int x) { return x * x; }

// CPP-AUTO-CONST — a const method cannot mutate members (auto-signal).
// (see `get() const` in self_class.cpp)

// CPP-ANN-OK — override on an already-clean fn is a no-op -> 0.
// ventouse: pure
int add_pure(int a, int b) { return a + b; }

// CPP-ANN-OVERRIDE — `// ventouse: pure` OVERWRITES the inferred dirt (unresolved call) -> forced 0.
// ventouse: pure
void trusted(int x) { external_io(x); }
