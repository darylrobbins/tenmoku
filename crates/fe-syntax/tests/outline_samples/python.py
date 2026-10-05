import functools

MAX_SIZE = 100
default_name = "x"
API_V2 = "/v2"


class Shape:
    SIDES = 0

    def __init__(self, name):
        self.name = name

    @property
    def area(self):
        return 0

    @functools.cache
    def perimeter(self):
        def side():
            return 1

        return side() * self.SIDES

    class Meta:
        def describe(cls):
            return "meta"


@functools.cache
def build(name):
    def inner():
        return Shape(name)

    return inner()


def main():
    LOCAL = 1
    return build("x")
