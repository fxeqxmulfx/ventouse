// ventouse C++ fixtures — edges (P1 surface).
#include <stdexcept>
#include <vector>

// CPP-EC-THROW — `throw` is control flow, NOT an effect (P1) -> clean 0.
int checked(int x) {
    if (x < 0) {
        throw std::runtime_error("neg");
    }
    return x;
}

// CPP-EC-STATICMUT — writing a mutable static member -> dirty 20 ; class 10.
struct Counter {
    static int total;
    void add() { total += 1; } // writes a mutable static -> dirty 20
};

// CPP-EC-LOCAL — a local owned vector mutated locally -> clean 0.
int build(int n) {
    std::vector<int> v;
    for (int i = 0; i < n; i++) { v.push_back(i); }
    return v.size();
}
