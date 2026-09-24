from app.core import Config, load


def build(path):
    cfg = Config(path)
    return cfg


def reload(path):
    return load(path)
