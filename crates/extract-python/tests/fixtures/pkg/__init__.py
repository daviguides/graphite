from .sub import service
from . import sub as subpkg


def init():
    service.build("root")
    subpkg.service.build("again")
