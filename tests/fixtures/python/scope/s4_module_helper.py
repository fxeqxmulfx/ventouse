# S4 — module helper used in one function: declaration is FREE of nesting -> scope_debt 0
def helper(x):
    return x + 1

def main():
    return helper(5)
