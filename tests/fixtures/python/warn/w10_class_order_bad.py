# W10 — class methods random order -> 2 forward-ref warnings (dirt 0)
"""Bad code: the same class but methods in random order. `handle` calls
`self._validate` and `self._transform`, both defined BELOW it -> 2 forward-ref
declare-before-use warnings. Logic is identical and pure (dirt 0) — only the
method order is bad.
"""


class Service:
    def __init__(self, data):
        self.data = data

    def handle(self, x):
        if self._validate(x):
            return self._transform(x)
        return 0

    def _validate(self, x):
        return x > 0

    def _transform(self, x):
        return x * 2
