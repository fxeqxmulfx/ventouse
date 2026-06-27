# W4 — mutual recursion -> no warning
def a(n):
    return b(n)

def b(n):
    return a(n)
