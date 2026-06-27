# W5 — default arg evaluated at def time -> warning
def g(a=later):
    return a

later = 1
