#!/usr/bin/env python3
"""Generates a chain of GNOME-Files ("Nautilus")-styled HTML pages showing a
directory's REAL contents (os.scandir, not fabricated), for
checkerboard-calibration/record.sh's "browse to the saved calibration file"
step.

Chrome's own built-in file:// directory listing (the previous approach) is
legible as exactly what it is -- a bare browser page -- not a native Linux
file manager. Automating the real GNOME Files app instead was tried and
abandoned (see checkerboard-calibration/POINTING-NOTES.md): AT-SPI hit
real, escalating isolation problems (a GApplication singleton service that
leaked a window onto the real desktop, then a redundant AT-SPI stack, then
an unexplained accessibility-bus connection failure). This script fakes the
LOOK of Nautilus instead, using this machine's real installed Yaru icon
theme, while remaining a real, CDP-queryable HTML page like everywhere else
in this pipeline.

Usage: render_nautilus_listing.py OUTPUT_DIR START_DIR [SUBDIR...]

Walks START_DIR, START_DIR/SUBDIR[0], START_DIR/SUBDIR[0]/SUBDIR[1], ...
and writes one page per level to OUTPUT_DIR/nautilus_0.html ..
nautilus_N.html. On each page, the one real directory entry matching that
level's SUBDIR gets a genuine <a href="file://.../nautilus_{i+1}.html">
around its name -- this is what keeps record.sh's existing
click_browser_element(needle, ancestor_tag="a") calls working unchanged.
Every other real entry is still listed (name + icon), just inert. The final
page (no more SUBDIRs left) has no forward link at all.

Prints the path to nautilus_0.html on stdout.
"""

import base64
import functools
import html
import os
import pathlib
import sys

ICON_THEME_BASES = [
    "/usr/share/icons/Yaru",
    "/usr/share/icons/Adwaita",
    "/usr/share/icons/hicolor",
]
ICON_SIZE_DIRS = ["48x48", "256x256", "scalable"]

FOLDER_ICON_REL = "places/folder.png"
MIME_ICON_RELS = {
    ".yaml": "mimetypes/application-x-yaml.png",
    ".yml": "mimetypes/application-x-yaml.png",
}
DEFAULT_FILE_ICON_REL = "mimetypes/text-x-generic.png"

SIDEBAR_ITEMS = [
    ("Recent", "places/folder-recent.png"),
    ("Starred", "places/folder.png"),
    ("Home", "places/user-home.png"),
    ("Documents", "places/folder-documents.png"),
    ("Downloads", "places/folder-download.png"),
    ("Music", "places/folder-music.png"),
    ("Pictures", "places/folder-pictures.png"),
    ("Videos", "places/folder-videos.png"),
    ("Trash", "places/user-trash.png"),
]

# Minimal inline-SVG glyphs, used only if no icon theme is installed at all.
_FALLBACK_FOLDER_SVG = (
    "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 48 48'>"
    "<path fill='#f6cd6c' d='M4 12h16l4 6h20v20H4z'/></svg>"
)
_FALLBACK_FILE_SVG = (
    "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 48 48'>"
    "<path fill='#e0e0e0' stroke='#999' d='M10 4h20l8 8v32H10z'/></svg>"
)


def _find_icon_bytes(relative_path):
    for base in ICON_THEME_BASES:
        for size_dir in ICON_SIZE_DIRS:
            for ext in (".png", ".svg"):
                candidate = pathlib.Path(base, size_dir, relative_path).with_suffix(ext)
                if candidate.is_file():
                    return candidate.read_bytes(), ext
    return None, None


@functools.lru_cache(maxsize=None)
def _icon_data_uri(relative_path, fallback_svg):
    data, ext = _find_icon_bytes(relative_path)
    if data is None:
        encoded = base64.b64encode(fallback_svg.encode("utf-8")).decode("ascii")
        return f"data:image/svg+xml;base64,{encoded}"
    mime = "image/png" if ext == ".png" else "image/svg+xml"
    encoded = base64.b64encode(data).decode("ascii")
    return f"data:{mime};base64,{encoded}"


def _icon_rel_for_entry(entry):
    if entry.is_dir():
        return FOLDER_ICON_REL, _FALLBACK_FOLDER_SVG
    ext = pathlib.Path(entry.name).suffix.lower()
    rel = MIME_ICON_RELS.get(ext, DEFAULT_FILE_ICON_REL)
    return rel, _FALLBACK_FILE_SVG


def _breadcrumb(current_dir, home_dir):
    try:
        rel = current_dir.relative_to(home_dir)
    except ValueError:
        return [str(current_dir)]
    parts = ["Home"]
    if str(rel) != ".":
        parts.extend(rel.parts)
    return parts


def _render_page(current_dir, home_dir, next_name, next_href):
    # Icons are declared once per distinct theme path as a CSS class and
    # referenced by class on every matching entry, rather than repeating a
    # full base64 data URI on every single <img> -- a real directory here
    # can have thousands of entries (this dev machine's own $HOME does), and
    # repeating a ~1-2KB icon blob per entry would bloat a single page to
    # multiple megabytes for no benefit.
    icon_classes = {}
    icon_css_rules = []

    def register_icon(rel_path, fallback_svg):
        if rel_path not in icon_classes:
            cls = f"ic{len(icon_classes)}"
            icon_classes[rel_path] = cls
            uri = _icon_data_uri(rel_path, fallback_svg)
            icon_css_rules.append(f".{cls} {{ background-image: url({uri}); }}")
        return icon_classes[rel_path]

    breadcrumb = _breadcrumb(current_dir, home_dir)
    breadcrumb_html = "".join(
        f"<span class='crumb{' current' if i == len(breadcrumb) - 1 else ''}'>"
        f"{html.escape(part)}</span><span class='crumb-sep'>&#9656;</span>"
        if i < len(breadcrumb) - 1
        else f"<span class='crumb current'>{html.escape(part)}</span>"
        for i, part in enumerate(breadcrumb)
    )

    sidebar_html = "".join(
        f"<div class='sidebar-item{' selected' if label == 'Home' and str(current_dir) == str(home_dir) else ''}'>"
        f"<div class='sidebar-icon {register_icon(icon_rel, _FALLBACK_FOLDER_SVG)}'></div>"
        f"<span>{html.escape(label)}</span></div>"
        for label, icon_rel in SIDEBAR_ITEMS
    )

    entries = sorted(
        os.scandir(current_dir),
        key=lambda e: (not e.is_dir(), e.name.lower()),
    )

    grid_items = []
    for entry in entries:
        icon_rel, fallback_svg = _icon_rel_for_entry(entry)
        icon_div = f"<div class='icon {register_icon(icon_rel, fallback_svg)}'></div>"
        name = html.escape(entry.name)
        if entry.name == next_name and next_href is not None:
            grid_items.append(
                f"<a class='item' href='{html.escape(next_href)}'>"
                f"{icon_div}<span class='label'>{name}</span></a>"
            )
        else:
            grid_items.append(
                f"<div class='item'>{icon_div}<span class='label'>{name}</span></div>"
            )
    grid_html = "".join(grid_items)
    icon_css = "\n  ".join(icon_css_rules)

    return f"""<!doctype html>
<html><head><meta charset="utf-8"><title>{html.escape(current_dir.name or "Home")}</title>
<style>
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0; height: 100vh; display: flex; flex-direction: column;
    font-family: "Ubuntu", "Cantarell", sans-serif; background: #fff; color: #222;
  }}
  .header {{
    display: flex; align-items: center; gap: 10px; padding: 8px 12px;
    background: #3b3b3b; color: #eee; flex-shrink: 0;
  }}
  .nav-btn {{
    width: 22px; height: 22px; border-radius: 4px; display: flex;
    align-items: center; justify-content: center; color: #999; font-size: 16px;
  }}
  .breadcrumb {{ display: flex; align-items: center; gap: 4px; margin-left: 8px; }}
  .crumb {{
    padding: 4px 10px; border-radius: 4px; background: #4d4d4d; font-size: 14px;
  }}
  .crumb.current {{ background: #5a5a5a; font-weight: bold; }}
  .crumb-sep {{ color: #888; font-size: 11px; }}
  .body {{ display: flex; flex: 1; min-height: 0; }}
  .sidebar {{
    width: 180px; flex-shrink: 0; background: #f2f2f2; border-right: 1px solid #ddd;
    padding: 8px 0; overflow-y: auto;
  }}
  .sidebar-item {{
    display: flex; align-items: center; gap: 8px; padding: 6px 14px; font-size: 14px;
  }}
  .sidebar-icon {{
    width: 20px; height: 20px; background-size: contain;
    background-repeat: no-repeat; background-position: center;
  }}
  .sidebar-item.selected {{ background: #d8e8fb; }}
  .content {{
    flex: 1; overflow-y: auto; padding: 20px;
    display: flex; flex-wrap: wrap; align-content: flex-start;
    gap: 4px;
  }}
  .item {{
    width: 100px; display: flex; flex-direction: column; align-items: center;
    padding: 10px 4px; border-radius: 6px; text-decoration: none; color: #222;
  }}
  .icon {{
    width: 64px; height: 64px; margin-bottom: 6px; background-size: contain;
    background-repeat: no-repeat; background-position: center;
  }}
  .item .label {{
    font-size: 12.5px; text-align: center; word-break: break-word; max-width: 96px;
  }}
  {icon_css}
</style></head>
<body>
  <div class="header">
    <div class="nav-btn">&#8592;</div>
    <div class="nav-btn">&#8594;</div>
    <div class="breadcrumb">{breadcrumb_html}</div>
  </div>
  <div class="body">
    <div class="sidebar">{sidebar_html}</div>
    <div class="content">{grid_html}</div>
  </div>
</body></html>
"""


def main():
    output_dir = pathlib.Path(sys.argv[1])
    start_dir = pathlib.Path(sys.argv[2]).resolve()
    subdirs = sys.argv[3:]

    output_dir.mkdir(parents=True, exist_ok=True)

    dirs = [start_dir]
    for subdir in subdirs:
        dirs.append(dirs[-1] / subdir)

    html_paths = [output_dir / f"nautilus_{i}.html" for i in range(len(dirs))]

    for i, current_dir in enumerate(dirs):
        if i < len(subdirs):
            next_name = subdirs[i]
            next_href = html_paths[i + 1].as_uri()
        else:
            next_name = None
            next_href = None
        page = _render_page(current_dir, start_dir, next_name, next_href)
        html_paths[i].write_text(page)

    print(html_paths[0])


if __name__ == "__main__":
    main()
