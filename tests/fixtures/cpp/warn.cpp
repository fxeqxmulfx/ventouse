// ventouse C++ fixtures — callees-before-callers inside a class (P5).
// (Free functions already require a prior declaration -> the compiler owns that ordering; we
//  warn on in-class method order, where a method may call a member declared later in the body.)

struct Service {
    int handle() { return helper(); } // CPP-W-BAD — calls a method declared BELOW -> warning
    int helper() { return 1; }
};

struct Tidy {
    int helper() { return 1; }
    int handle() { return helper(); } // CPP-W-GOOD — callee above caller -> 0 warnings
};
