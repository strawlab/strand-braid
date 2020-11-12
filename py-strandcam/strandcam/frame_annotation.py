class Point(object):
    def __init__(self, x, y):
        self.x = x
        self.y = y


class FrameAnnotation(object):
    def __init__(self, points):
        self._points = points
