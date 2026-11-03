import lib
from lib import Circle, Point, Shape, Square, Sub, alias, decorated, over, plain
from reexport import plain as again


def run(s: Shape, u: Circle | Square, flag: bool):
    c = Circle(1)
    c.area()
    s.area()
    u.area()
    (c if flag else Square()).area()
    Sub(1).area()
    Point(1)
    over(1)
    decorated()
    alias()
    again()
    lib.plain()
    Circle.make()
    Circle.helper()
    c.size
    c.size = 2
    print(plain)
    make_it().area()
    [c][0].area()
    getattr(c, "area")()


def make_it() -> Circle | Square:
    return Circle(1)


top = plain()
