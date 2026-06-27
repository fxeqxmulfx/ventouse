// ventouse C++ fixtures — class/struct unit (P1/P2). The constructor is the method named after
// the class; member initialization there is clean.
struct S {
    int n;
    S(int n_) : n(n_) {}          // CPP-CTOR — member init in the ctor -> clean 0
    void set(int v) { n = v; }    // CPP-SET — non-const method writes a member -> dirty 20
    int get() const { return n; } // CPP-GET — const method cannot mutate -> clean 0
};
// S has a dirty method (set) -> the class is dirty: base 10 + set 20 = 30.
