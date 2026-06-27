# W7 — call chain, callees declared above callers (bottom-up) -> 0 warnings
"""Good: a call chain f1 -> f2 -> f3 -> f4 -> f5 with every callee declared
before its caller (bottom-up). Declare-before-use is satisfied: 0 warnings.
All functions are pure (arithmetic + project-clean calls): dirt 0.
"""


def f5(x):
    return x + 1


def f4(x):
    return f5(x) + 1


def f3(x):
    return f4(x) + 1


def f2(x):
    return f3(x) + 1


def f1(x):
    return f2(x) + 1
