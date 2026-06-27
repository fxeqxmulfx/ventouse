# W3 — forward function reference -> warning
def f():
    return g()

def g():
    return 1
