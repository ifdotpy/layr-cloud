# Layr Cloud UI mockup

A static, clickable mockup of the Layr Cloud web UI with fake data. It follows
[`docs/ui-mockup-spec.md`](../docs/ui-mockup-spec.md).

Open `index.html` (organization) or `code.html` (project `shop`) in a browser. No server and no build
are needed. Light and dark follow the system setting.

The HTML files are generated. Edit the generator, then run:

```sh
python3 mockup/tools/build.py   # writes every page
python3 mockup/tools/check.py   # links, HTML, vocabulary, external resources
```
