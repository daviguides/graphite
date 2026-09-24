from app.service import build


def test_build():
    assert build("p") is not None
