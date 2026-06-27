# W8 — same chain top-down -> 4 forward-ref warnings (dirt 0)
"""Bad code: the same f1 -> f2 -> f3 -> f4 -> f5 chain, but every caller is
declared ABOVE its callee (top-down). Each call is a forward reference to a
function defined later -> 4 declare-before-use warnings (f1->f2, f2->f3,
f3->f4, f4->f5). Logic is identical and pure, so dirt is still 0 — only the
ordering is bad.
"""


def f1(x):
    return f2(x) + 1


def f2(x):
    return f3(x) + 1


def f3(x):
    return f4(x) + 1


def f4(x):
    return f5(x) + 1


def f5(x):
    return x + 1
