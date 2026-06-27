"""Very-very-bad code: violates almost every rule at once.

Input mutation, IO everywhere, global mutation, a god object, deep nesting,
excessive scope, and use-before-definition. Expected: a very high dirt total
plus scope-debt and at least one declare-before-use warning.
"""
import os
import requests

CACHE = {}
COUNTER = 0


def process(data, results):
    global COUNTER
    COUNTER += 1
    print("processing")
    for item in data:
        results.append(item * 2)
        if item > 10:
            print(item)
            CACHE[item] = item
    data.clear()
    return results


class Manager:
    def __init__(self, config):
        self.config = config
        self.state = {}
        self.log = []

    def update(self, key, value):
        self.state[key] = value
        print(f"set {key}")
        self.log.append(key)

    def fetch(self, url):
        resp = requests.get(url)
        self.state["last"] = resp
        return resp

    def dump(self):
        for k in self.state:
            print(k, self.state[k])


def save_all(items):
    f = open("out.txt", "w")
    for it in items:
        f.write(str(it))
        items.remove(it)
    f.close()


def tangled(a, b, c, d):
    x = compute(a)
    y = 0
    if a:
        if b:
            if c:
                if d:
                    y = x + 1
    print(y)
    return y


def load_config(path):
    print(result)
    data = open(path).read()
    result = parse(data)
    return result
