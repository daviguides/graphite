import pytest

from pkg.sub.service import build


@pytest.fixture
def svc():
    return build("t")


def test_build(svc):
    assert svc.name == "t"


class TestService:
    def test_label(self, svc):
        assert svc.label == "t"

    def helper(self):
        return build("h")
