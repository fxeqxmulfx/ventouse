"""Concentrated very-bad code: ONE function breaking the maximum number of rules
in the fewest lines — IO, input mutation, global mutation, a dirty call,
excessive scope, and use-before-definition.
"""
G = 0


def fubar(items, path):
    global G
    print(early)               # IO + use-before-def: `early` read before line 5
    G += 1                     # global mutation
    items.append(path)         # input mutation (param)
    cfg = open(path).read()    # IO; `cfg` declared here, used only deep below
    early = path               # binds `early` (read above)
    if items:
        if path:
            parse(cfg)         # unknown call -> dirty; `cfg` used 2 levels deep
