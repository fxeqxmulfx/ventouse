// C++ — see DESIGN per-language examples
#include <vector>
#include <iostream>
void push_it(std::vector<int>& v, int x) { v.push_back(x); }   // non-const ref -> dirty
int size_of(const std::vector<int>& v) { return v.size(); }    // const ref + const method -> clean
void log_it(int x) { std::cout << x; }                         // IO -> dirty
struct S {
    int n;
    S(int n_) : n(n_) {}          // constructor -> clean
    void set(int v) { n = v; }    // non-const method mutates member -> dirty
    int get() const { return n; } // const method -> clean
};
constexpr int dbl(int x) { return x * 2; }                     // constexpr auto-signal -> clean
