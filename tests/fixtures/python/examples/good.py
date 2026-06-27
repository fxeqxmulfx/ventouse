"""Good code: a functional core with a thin imperative shell.

All computation is pure; the single side effect (printing) is isolated to `main`.
Expected: dirt is confined to one of six entities; everything else scores 0.
"""
from dataclasses import dataclass


@dataclass(frozen=True)
class Order:
    items: tuple
    discount: float


def subtotal(order):
    return sum(price for _, price in order.items)


def apply_discount(amount, discount):
    return amount * (1 - discount)


def total(order):
    return apply_discount(subtotal(order), order.discount)


def to_dollars(cents):
    return cents / 100


def main(order):
    print(to_dollars(total(order)))
