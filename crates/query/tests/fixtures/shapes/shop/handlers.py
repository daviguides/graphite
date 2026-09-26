from shop.base import Handler
from shop.util import fmt


class JsonHandler(Handler):
    def handle(self, req):
        return fmt(req)


class XmlHandler(JsonHandler):
    def handle(self, req):
        return fmt(fmt(req))
