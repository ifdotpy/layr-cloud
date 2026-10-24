"""Write every mockup page into mockup/. Run: python3 mockup/tools/build.py"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
OUT = os.path.dirname(HERE)

import pages_code, pages_change, pages_ops, pages_settings, pages_org, pages_overlays  # noqa: E402

pages = {}
for m in (pages_code, pages_change, pages_ops, pages_settings, pages_org, pages_overlays):
    pages.update(m.PAGES)
for name, fn in pages.items():
    with open(os.path.join(OUT, name), "w", encoding="utf-8") as f:
        f.write(fn())
print(f"{len(pages)} pages")
