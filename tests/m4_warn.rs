//! M4 — declare-before-use warnings (P5): callees-before-callers flavor (forward references
//! between same-scope siblings; unavoidable cycles exempt). DESIGN W3, W4, W7, W8, W9, W10, EC14
//! plus use-before-binding for variables (W1/W2/W5/W6). The broader references model (decorators /
//! bases / defaults / bare-name / self-member references) is covered in `m4_references.rs`.

use ventouse::lang::python::warnings_of;

#[test]
fn w3_forward_function_reference() {
    // f calls g defined later (non-cyclic) -> 1 warning
    let src = "def f():\n    return g()\n\ndef g():\n    return 1\n";
    assert_eq!(warnings_of(src), 1);
}

#[test]
fn w4_mutual_recursion_exempt() {
    let src = "def a(n):\n    return b(n)\n\ndef b(n):\n    return a(n)\n";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn ec14_self_recursion_exempt() {
    let src = "def f(n):\n    if n <= 0:\n        return 0\n    return f(n - 1)\n";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn w7_chain_declared_bottom_up_clean() {
    // each callee declared ABOVE its caller -> 0 warnings
    let src = "\
def f5():
    return 5

def f4():
    return f5()

def f3():
    return f4()

def f2():
    return f3()

def f1():
    return f2()
";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn w8_chain_declared_top_down_4_warnings() {
    // each caller above its callee -> 4 forward references
    let src = "\
def f1():
    return f2()

def f2():
    return f3()

def f3():
    return f4()

def f4():
    return f5()

def f5():
    return 5
";
    assert_eq!(warnings_of(src), 4);
}

#[test]
fn w9_class_methods_ordered_clean() {
    let src = "\
class S:
    def _validate(self):
        return 1
    def _transform(self):
        return 2
    def handle(self):
        return self._validate() + self._transform()
";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn w10_class_methods_random_order_warns() {
    // handle declared above the helpers it calls -> 2 forward references
    let src = "\
class S:
    def handle(self):
        return self._validate() + self._transform()
    def _validate(self):
        return 1
    def _transform(self):
        return 2
";
    assert_eq!(warnings_of(src), 2);
}

#[test]
fn callee_above_caller_is_clean() {
    let src = "def helper(x):\n    return x + 1\n\ndef f(x):\n    return helper(x)\n";
    assert_eq!(warnings_of(src), 0);
}

// --- use-before-binding (variables) -----------------------------------------------------

#[test]
fn w1_local_use_before_assignment() {
    let src = "def f():\n    print(x)\n    x = 1\n";
    assert_eq!(warnings_of(src), 1);
}

#[test]
fn w2_module_forward_ref() {
    let src = "a = b\nb = 2\n";
    assert_eq!(warnings_of(src), 1); // warning on `b`
}

#[test]
fn w5_default_arg_evaluated_at_def_time() {
    let src = "def g(a=later):\n    return a\n\nlater = 1\n";
    assert_eq!(warnings_of(src), 1); // `later` used at def-time before its module binding
}

#[test]
fn w6_aug_assign_before_binding() {
    let src = "def f():\n    x += 1\n    x = 0\n";
    assert_eq!(warnings_of(src), 1);
}

#[test]
fn bind_then_use_is_clean() {
    let src = "def f():\n    x = 1\n    return x\n";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn param_use_is_clean() {
    // a parameter is bound on entry — using it is never use-before-binding
    let src = "def f(x):\n    return x + 1\n";
    assert_eq!(warnings_of(src), 0);
}

#[test]
fn use_before_binding_across_blocks() {
    // x used in an if-body before its assignment after the if -> 1 declare-before-use warning.
    let src = "def f(c):\n    if c:\n        print(x)\n    x = 1\n";
    assert_eq!(warnings_of(src), 1);
}
