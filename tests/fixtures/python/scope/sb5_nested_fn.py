# SB5 — declaring a function is FREE (no nesting penalty); helper has no dependencies -> scope_debt 0
def f(flag):
    def helper():
        return 1
    if flag:
        return helper()
    return 0
