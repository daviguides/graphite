"""Fixture covering every construct the extractor handles."""
import os
import os.path
import json as j
from typing import Optional
from . import helpers
from .models import User as U, Account
from ..core.base import BaseService
from .star import *

__all__ = ["Service", "build", "Account"]


def build(name: str) -> "Service":
    svc = Service(name)
    svc.start()
    helpers.log("built")
    print(os.getcwd(), j.dumps({}))
    return svc


def _private():
    return build("x")


class Service(BaseService, metaclass=Meta):
    def __init__(self, name: Optional[str] = None):
        super().__init__()
        self.name = name

    @property
    def label(self) -> str:
        return self.name

    @label.setter
    def label(self, value):
        self.name = value

    async def start(self):
        self._boot()
        self.missing()
        Service.stop(self)
        U.load(self.name)

    def _boot(self):
        def inner():
            return self.label

        return inner()

    @staticmethod
    def stop(svc):
        os.path.join("a", "b")
        factory()()


class Nested(Service):
    class Config:
        debug = True


@app.route("/x")
def handler():
    return undefined_thing()
