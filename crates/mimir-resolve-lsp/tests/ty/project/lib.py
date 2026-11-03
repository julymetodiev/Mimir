from dataclasses import dataclass
from typing import Protocol, overload


class Shape(Protocol):
    def area(self) -> int: ...


class Circle:
    def __init__(self, r: int):
        self.r = r

    def area(self) -> int:
        return self.r

    @property
    def size(self) -> int:
        return self.r

    @size.setter
    def size(self, v: int) -> None:
        self.r = v

    @classmethod
    def make(cls) -> "Circle":
        return cls(1)

    @staticmethod
    def helper() -> int:
        return 1


class Square:
    def area(self) -> int:
        return 2


class Sub(Circle):
    def area(self) -> int:
        return super().area()


@dataclass
class Point:
    x: int


@overload
def over(a: int) -> int: ...
@overload
def over(a: str) -> str: ...
def over(a):
    return a


def deco(f):
    return f


@deco
def decorated() -> int:
    return 1


def untyped(value):
    return value.strip()


def plain() -> int:
    return 1


alias = plain
