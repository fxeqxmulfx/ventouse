# SB1-SB4 — introduced targets at natural block: i/h/e/j levels_excess 0, no placement scope-debt
def f(xs):
    for i in xs:          # SB1 for-target
        print(i)
    with open("x") as h:  # SB2 with-as
        h.read()
    try:
        pass
    except E as e:        # SB3 except-as (deleted after)
        print(e)
    return [j for j in xs]  # SB4 comprehension target (own scope)
