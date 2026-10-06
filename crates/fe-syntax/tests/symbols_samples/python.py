import os

VERSION = "1.0"
counter = 0
path: str = os.getcwd()
first, second = 1, 2


class Config:
    timeout = 30
    name: str
    retries: int = 3
    DEFAULTS = {}

    def __init__(self):
        self.count = 0
        local = 1

    class Inner:
        depth = 0


def main():
    result = Config()
    return result
