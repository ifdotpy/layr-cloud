"""Check the mockup: links resolve, tags balance, no external resources but fonts, no Git words."""
import glob
import os
import re
import sys
from html.parser import HTMLParser

OUT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VOID = {"meta", "link", "input", "br", "hr", "img", "source", "wbr"}
WORDS = re.compile(r"\b((?i:backup|pull request|commit|commits|branch|branches|stash|cherry-pick|rebase|revert)|PR|PRs|HEAD)\b")
ALLOWED = ["Git commit"]


class P(HTMLParser):
    def __init__(self):
        super().__init__()
        self.stack, self.errors, self.links, self.ext, self.text, self.ids = [], [], [], [], [], set()

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if "id" in a:
            self.ids.add(a["id"])
        for k in ("href", "src"):
            v = a.get(k)
            if v and v.startswith("http"):
                self.ext.append(v)
            elif v and k == "href":
                self.links.append(v)
        for k in ("data-copy", "placeholder", "value", "aria-label", "title"):
            if a.get(k):
                self.text.append(a[k])
        if tag not in VOID:
            self.stack.append((tag, self.getpos()))

    def handle_endtag(self, tag):
        if tag in VOID:
            return
        if not self.stack or self.stack[-1][0] != tag:
            self.errors.append(f"line {self.getpos()[0]}: </{tag}> but open {self.stack[-1] if self.stack else None}")
            for i in range(len(self.stack) - 1, -1, -1):
                if self.stack[i][0] == tag:
                    del self.stack[i:]
                    break
        else:
            self.stack.pop()

    def handle_data(self, d):
        self.text.append(d)


bad = 0
files = sorted(glob.glob(os.path.join(OUT, "*.html")))
nlinks = 0
for f in files:
    p = P()
    p.feed(open(f, encoding="utf-8").read())
    name = os.path.basename(f)
    for err in p.errors + [f"unclosed {t} at {pos}" for t, pos in p.stack]:
        print(f"{name}: HTML {err}"); bad += 1
    for l in p.links:
        nlinks += 1
        target = l.split("#")[0]
        if target and not os.path.exists(os.path.join(OUT, target)):
            print(f"{name}: broken link {l}"); bad += 1
    for x in p.ext:
        if not x.startswith(("https://fonts.googleapis.com", "https://fonts.gstatic.com")):
            print(f"{name}: external {x}"); bad += 1
    for t in p.text:
        for m in WORDS.finditer(t):
            ctx = t[max(0, m.start() - 20): m.end() + 20]
            if not any(a in ctx for a in ALLOWED):
                print(f"{name}: word '{m.group(0)}' in: {ctx.strip()}"); bad += 1
print(f"{len(files)} pages, {nlinks} links, {bad} problems")
sys.exit(1 if bad else 0)
