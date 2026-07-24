#!/usr/bin/env python3
"""Builds a small standalone HTML page that displays a file's real content,
dispatched by file extension: text-like files (the default -- covers this
scenario's own .yaml, and anything unrecognized, since showing arbitrary
bytes as text is always safe whereas guessing image/video wrong would just
render a broken tag) get wrapped in a <pre>; images get an <img>; videos get
a <video>.

Exists so record.sh can show a file's real content in an isolated Chrome
window without depending on Chrome's own native file-open handling (which
downloads unrecognized types like .yaml instead of displaying them -- see
checkerboard-calibration/POINTING-NOTES.md) or a native desktop app (no CDP,
so no way to verify/point at anything inside it -- see the same notes for
why AT-SPI wasn't used instead). Because the result is plain HTML rendered
by Chrome, cdp_locate.py/point_at_browser_text work on its content exactly
like everywhere else in this pipeline.

Usage: render_file_viewer.py FILE_PATH OUTPUT_HTML_PATH
"""

import html
import pathlib
import sys

IMAGE_EXTENSIONS = {".png", ".jpg", ".jpeg", ".gif", ".bmp"}
VIDEO_EXTENSIONS = {".mp4", ".webm", ".mov"}


def main() -> None:
    file_path = pathlib.Path(sys.argv[1]).resolve()
    output_path = pathlib.Path(sys.argv[2])
    ext = file_path.suffix.lower()
    title = html.escape(file_path.name)
    file_uri = file_path.as_uri()

    if ext in IMAGE_EXTENSIONS:
        body = f'<img src="{file_uri}" style="max-width:100%">'
    elif ext in VIDEO_EXTENSIONS:
        body = f'<video src="{file_uri}" controls style="max-width:100%"></video>'
    else:
        content = file_path.read_text(errors="replace")
        body = f"<pre>{html.escape(content)}</pre>"

    output_path.write_text(
        "<!doctype html><html><head><meta charset='utf-8'>"
        f"<title>{title}</title>"
        "<style>body{font-family:monospace;font-size:22px;margin:32px;"
        "background:white;color:black}</style>"
        f"</head><body>{body}</body></html>"
    )


if __name__ == "__main__":
    main()
