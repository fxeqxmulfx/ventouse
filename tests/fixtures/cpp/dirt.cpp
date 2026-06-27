// ventouse C++ fixtures — Dirt (P1 effects, P2 contagion). dirty = 10 + 10*owned_lines.
#include <vector>
#include <iostream>

// CPP-A — pure arithmetic -> clean 0
int add(int a, int b) { return a + b; }

// CPP-B — IO (std::cout) -> dirty 20 (owned 1)
void log_it(int x) { std::cout << x; }

// CPP-C — input mutation through a non-const ref -> dirty 20 (owned 1)
void push_it(std::vector<int>& v, int x) { v.push_back(x); }

// CPP-C-CLEAN — const ref + const method -> cannot mutate -> clean 0
int size_of(const std::vector<int>& v) { return v.size(); }

// CPP-GLOBAL — non-const global write -> dirty ; const read -> clean.
const int MAX = 10;
int g_count = 0;
int read_max() { return MAX; } // const read -> clean 0
void bump() { g_count += 1; }  // non-const global write -> dirty 20 (owned 1)

// CPP-D — contagion (P2): wlog dirty 20, run infected (owned 2) -> 30. total 50.
void wlog(int m) { std::cout << m; }
void run(int d) {
    int r = d * 2;
    wlog(r);
}
