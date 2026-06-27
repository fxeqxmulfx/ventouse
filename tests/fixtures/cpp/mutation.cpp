// ventouse C++ fixtures — Mutation (P1). const-correctness is resolved precisely via libclang.
#include <vector>

// CPP-MUT-PTR — write through a non-const pointer param -> dirty 20
void set_ptr(int* p) { *p = 5; }

// CPP-MUT-CONSTPTR — a const pointer param cannot mutate the pointee -> clean 0
int read_ptr(const int* p) { return *p; }

// CPP-MUT-BYVAL — a by-value param is a copy; mutating it is local -> clean 0
int inc(int x) { x++; return x; }

// CPP-MUT-ARROW — member write through a pointer param -> dirty 20
struct Obj { int x; };
void set_obj(Obj* o, int v) { o->x = v; }
