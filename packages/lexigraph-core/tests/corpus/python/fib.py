# Indentation-sensitive Python with comments and triple-quoted strings.
"""Module-level docstring.

Multiple lines.
"""

from typing import Iterable


def fib(n: int) -> Iterable[int]:
    """Yield the first n Fibonacci numbers."""
    a, b = 0, 1
    for _ in range(n):
        yield a
        a, b = b, a + b  # trailing


class Counter:
    def __init__(self) -> None:
        self.n = 0  # init

    def inc(self, by: int = 1) -> None:
        # increment
        self.n += by


if __name__ == "__main__":
    print(list(fib(10)))
