# S3 — x declared too early: `r` is wedged between x and its use (`s` is co-used at the return -> exempt) -> 10
def f(p, q):
    x = 0
    r = p + q
    s = r * 2
    return x + s
