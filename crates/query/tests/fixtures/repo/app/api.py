from app import service


def handler(req):
    return service.build(req)


def unrelated(x):
    return x.get("k")
