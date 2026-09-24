def load(path):
    return open(path).read()


def parse(text):
    return text.split()


class Config:
    def __init__(self, path):
        self.data = parse(load(path))

    def get(self, key):
        return self.data
