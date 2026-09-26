from shop.util import render


def serve(req):
    return render(req)


def serve_twice(req):
    render(req)
    return render(req)
