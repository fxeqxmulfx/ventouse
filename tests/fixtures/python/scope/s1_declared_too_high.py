# S1 — x used only inside the `if` -> levels_excess 1 -> scope_debt 10 (no wedge: `if` header is not a definition)
def f(flag):
    x = 1
    if flag:
        print(x)
