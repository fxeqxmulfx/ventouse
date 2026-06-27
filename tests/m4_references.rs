//! The references model (L5): a forward reference to a same-scope definition warns, regardless of
//! how it is referenced — a call, a bare name, a decorator, a base class, a default argument, or a
//! `self.`/`cls.` member. Header references (decorator / base / default) are resolved in the
//! enclosing scope but ATTRIBUTED to the entity being defined (not `<module>`). Type annotations
//! are excluded, so recursive/forward types stay clean.

use ventouse::core::{Category, DeclOrder, Reason, Weights};
use ventouse::lang::python::analyze_source;

/// Entities of the declare-before-use warnings, in finding order.
fn decl_warn_entities(src: &str) -> Vec<String> {
    analyze_source(src, "t.py", &Weights::default())
        .into_iter()
        .filter(|f| f.category == Category::DeclBeforeUse)
        .map(|f| f.entity)
        .collect()
}

/// (entity, reason) of declare-before-use warnings under a given reading convention.
fn order_warnings(src: &str, order: DeclOrder) -> Vec<(String, Reason)> {
    analyze_source(src, "t.py", &Weights { order, ..Weights::default() })
        .into_iter()
        .filter(|f| f.category == Category::DeclBeforeUse)
        .map(|f| (f.entity, f.reason))
        .collect()
}

#[test]
fn declare_order_direction_is_a_flag() {
    // `caller` references `callee` defined ABOVE it. Bottom-up (default) is happy; top-down (stepdown,
    // overview-first) wants the callee BELOW, so it flags `caller` instead.
    let src = "def callee():\n    return 1\n\ndef caller():\n    return callee()\n";
    assert!(order_warnings(src, DeclOrder::BottomUp).is_empty());
    assert_eq!(
        order_warnings(src, DeclOrder::TopDown),
        [("caller".to_string(), Reason::ForwardRef { callee: "callee".into(), below: false })]
    );

    // The mirror image: callee defined BELOW its caller — bottom-up warns, top-down is happy.
    let rev = "def caller():\n    return callee()\n\ndef callee():\n    return 1\n";
    assert_eq!(
        order_warnings(rev, DeclOrder::BottomUp),
        [("caller".to_string(), Reason::ForwardRef { callee: "callee".into(), below: true })]
    );
    assert!(order_warnings(rev, DeclOrder::TopDown).is_empty());
}

#[test]
fn mutual_recursion_is_exempt_in_both_directions() {
    // a<->b cycle: unavoidable, so neither convention warns.
    let src = "def a(n):\n    return b(n)\n\ndef b(n):\n    return a(n)\n";
    assert!(order_warnings(src, DeclOrder::BottomUp).is_empty());
    assert!(order_warnings(src, DeclOrder::TopDown).is_empty());
}

#[test]
fn bare_decorator_forward_ref_attributed_to_entity() {
    // @deco (no call) referencing a function below -> warning ON `view`, not `<module>`.
    let src = "@deco\ndef view():\n    return 1\n\ndef deco(f):\n    return f\n";
    assert_eq!(decl_warn_entities(src), ["view"]);
}

#[test]
fn call_decorator_is_consistent_with_bare() {
    // @deco() must behave the same as bare @deco (the call parens are irrelevant to the reference).
    let bare = "@deco\ndef view():\n    return 1\n\ndef deco(f):\n    return f\n";
    let call = "@deco()\ndef view():\n    return 1\n\ndef deco():\n    return lambda f: f\n";
    assert_eq!(decl_warn_entities(bare), decl_warn_entities(call));
    assert_eq!(decl_warn_entities(call), ["view"]);
}

#[test]
fn decorator_above_is_clean() {
    let src = "def deco(f):\n    return f\n\n@deco\ndef view():\n    return 1\n";
    assert!(decl_warn_entities(src).is_empty());
}

#[test]
fn base_class_forward_ref_attributed_to_class() {
    let src = "class C(Base):\n    pass\n\nclass Base:\n    pass\n";
    assert_eq!(decl_warn_entities(src), ["C"]);
}

#[test]
fn default_arg_forward_ref_attributed_to_function() {
    let src = "def f(x=helper):\n    return x\n\ndef helper():\n    return 1\n";
    assert_eq!(decl_warn_entities(src), ["f"]);
}

#[test]
fn bare_name_sibling_forward_ref() {
    // a function returning a sibling defined below (referenced, not called) still warns.
    let src = "def build():\n    return handler\n\ndef handler():\n    return 1\n";
    assert_eq!(decl_warn_entities(src), ["build"]);
}

#[test]
fn self_member_reference_not_call_forward() {
    // `cb = self.b` references method `b` below WITHOUT calling it -> warning on `C.a`.
    let src = "class C:\n    def a(self):\n        cb = self.b\n        return cb\n    def b(self):\n        return 1\n";
    assert_eq!(decl_warn_entities(src), ["C.a"]);
}

#[test]
fn recursive_type_annotation_is_clean() {
    // Annotations are not extracted, so a forward/recursive type does not warn.
    let src = "def link(n: Node) -> Node:\n    return n\n\nclass Node:\n    pass\n";
    assert!(decl_warn_entities(src).is_empty());
}

#[test]
fn header_reference_cycle_is_exempt() {
    // mutual references via headers form a cycle -> exempt (like mutual recursion).
    let src = "@b\ndef a():\n    return 1\n\n@a\ndef b(f):\n    return f\n";
    assert!(decl_warn_entities(src).is_empty());
}

#[test]
fn classmethod_cls_member_forward_ref() {
    let src = "class C:\n    @classmethod\n    def a(cls):\n        return cls.b()\n    @classmethod\n    def b(cls):\n        return 1\n";
    assert_eq!(decl_warn_entities(src), ["C.a"]);
}

#[test]
fn nested_decorated_function_attribution() {
    // @inner_deco on a function nested in `outer`, inner_deco defined below -> warning on outer.f.
    let src = "def outer():\n    @inner_deco\n    def f():\n        return 1\n    def inner_deco(g):\n        return g\n    return f\n";
    assert_eq!(decl_warn_entities(src), ["outer.f"]);
}

#[test]
fn dotted_decorator_on_non_def_is_clean() {
    // @app.route — `app` is a module value / import, not a same-module definition -> no edge.
    let src = "import flask\napp = flask.Flask(__name__)\n\n@app.route(\"/\")\ndef index():\n    return \"ok\"\n";
    assert!(decl_warn_entities(src).is_empty());
}

#[test]
fn inherited_self_method_is_not_a_forward_ref() {
    // self.save() resolves only within THIS class body; an inherited method is not found -> no edge.
    let src = "class Base:\n    def save(self):\n        return 1\nclass C(Base):\n    def run(self):\n        return self.save()\n";
    assert!(decl_warn_entities(src).is_empty());
}

#[test]
fn bare_call_in_method_resolves_to_module_function() {
    // `helper()` (no self) in a method is the MODULE helper (defined above) -> clean, NOT C.helper.
    let src = "def helper():\n    return 0\nclass C:\n    def helper(self):\n        return 1\n    def m(self):\n        return helper()\n";
    assert!(decl_warn_entities(src).is_empty());
}

#[test]
fn def_then_value_rebind_is_clean() {
    // `def f` then `f = 5` then `g = f` — f is referenced above g, no forward ref, no false debt.
    let src = "def f():\n    return 1\nf = 5\ng = f\n";
    assert!(decl_warn_entities(src).is_empty());
}

#[test]
fn nested_class_method_member_resolution() {
    // a method of a NESTED class resolves self.helper within that (inner) class body.
    let src = "class Outer:\n    class Inner:\n        def m(self):\n            return self.helper()\n        def helper(self):\n            return 1\n";
    assert_eq!(decl_warn_entities(src), ["Outer.Inner.m"]);
}

#[test]
fn class_name_reference_forward() {
    // `C.build()` references the class C (defined below) by name -> warning on the referrer.
    let src = "def make():\n    return C.build()\nclass C:\n    @staticmethod\n    def build():\n        return 1\n";
    assert_eq!(decl_warn_entities(src), ["make"]);
}

#[test]
fn builtin_decorator_is_clean() {
    // @property etc. are builtins, not same-module definitions -> no edge.
    let src = "class C:\n    @property\n    def v(self):\n        return 1\n";
    assert!(decl_warn_entities(src).is_empty());
}

#[test]
fn call_default_forward_ref() {
    let src = "def f(x=helper()):\n    return x\ndef helper():\n    return 1\n";
    assert_eq!(decl_warn_entities(src), ["f"]);
}

#[test]
fn data_declared_below_its_user_warns() {
    // a module constant read by a function but declared below it -> declare-order warning on the
    // function (module/class data is a definition: declare it above its users).
    let src = "def fetch():\n    return get(TIMEOUT)\nTIMEOUT = 30\n";
    assert_eq!(decl_warn_entities(src), ["fetch"]);
}

#[test]
fn data_declared_above_its_user_is_clean() {
    let src = "TIMEOUT = 30\ndef fetch():\n    return get(TIMEOUT)\n";
    assert!(decl_warn_entities(src).is_empty());
}

#[test]
fn data_rhs_forward_ref_is_attributed_to_the_constant() {
    // a constant's RHS reference belongs to the constant (not the module): TIMEOUT = DEFAULT*2 with
    // DEFAULT below -> declare-order warning on TIMEOUT.
    let src = "TIMEOUT = DEFAULT * 2\nDEFAULT = 10\n";
    assert_eq!(decl_warn_entities(src), ["TIMEOUT"]);
}

#[test]
fn local_assignment_rhs_belongs_to_the_function() {
    // a function-local `x = helper()` is NOT a data definition: its RHS reference belongs to the
    // enclosing function, so `f` (not `f.x`) depends on `helper` -> forward-ref warning on `f`.
    let src = "def f():\n    x = helper()\n    return x\n\ndef helper():\n    return 1\n";
    assert_eq!(decl_warn_entities(src), ["f"]);
}

// --- newly-read expression forms (lambda / comprehensions / f-strings / yield) ----------

#[test]
fn fstring_interpolation_is_a_reference() {
    // an f-string `{expr}` is a reference in the current scope (declare-order applies).
    let src = "def f():\n    return f\"{compute()}\"\n\ndef compute():\n    return 1\n";
    assert_eq!(decl_warn_entities(src), ["f"]);
}

#[test]
fn comprehension_iterable_is_a_reference() {
    // the first iterable is evaluated in the ENCLOSING scope, so `source` is a reference of `use`.
    let src = "def use():\n    return [x for x in source()]\n\ndef source():\n    return []\n";
    assert_eq!(decl_warn_entities(src), ["use"]);
}

#[test]
fn yield_value_is_a_reference() {
    let src = "def g():\n    yield produce()\n\ndef produce():\n    return 1\n";
    assert_eq!(decl_warn_entities(src), ["g"]);
}

#[test]
fn match_subject_is_a_reference() {
    // the match subject resolves in the enclosing scope — `classify` is a reference of `route`.
    let src = "def route(r):\n    match classify(r):\n        case _:\n            return 1\n\ndef classify(x):\n    return x\n";
    assert_eq!(decl_warn_entities(src), ["route"]);
}
