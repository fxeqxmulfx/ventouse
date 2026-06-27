# W9 — class methods ordered (callee above caller) -> 0 warnings
"""Good: methods ordered callees-before-callers. Each `self.m` reference points
to a method declared above it -> 0 declare-before-use warnings. All methods are
pure (no IO, no input/global mutation; ctor self-mutation is exempt) -> dirt 0.
"""


class Service:
    def __init__(self, data):
        self.data = data

    def _validate(self, x):
        return x > 0

    def _transform(self, x):
        return x * 2

    def handle(self, x):
        if self._validate(x):
            return self._transform(x)
        return 0
