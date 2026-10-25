import zlib
from lib import *
from pages_info import disk_bars

_rd = []

AGENTS = [f"agent-{i}" for i in range(1, 17)]


def line_switcher(current="main", mid="m-line"):
    lines = [("main", "protected"), ("topic-checkout", "bob"), ("feature-coupons", "alice"),
             ("fix-tax-rounding", "bob")] + [(a, "alice") for a in AGENTS[:6]] + [("agent-7 … agent-16", "10 more")]
    refs = [("env/prod", "1.4.2"), ("env/staging", "a91c"), ("release/1.4.2", ""), ("release/1.4.1", ""),
            ("pins/audit-2026-10", "pin"), ("holds/legal-hold-17", "hold"), ("leases/agent-3", "2 h left")]
    views = [("views/9f2c41", "main + secrets")]

    def items(xs):
        return "".join(f'<a href="code.html">{e(a)}<span>{e(b)}</span></a>' for a, b in xs)
    return f'''<div class="menu-wrap"><button class="btn" type="button" data-menu="{mid}"><span class="muted">line:</span> <span class="mono">{e(current)}</span> {icon("chev")}</button>
<div class="menu switcher" id="{mid}">
<div class="sw-tabs" role="tablist">
<button type="button" role="tab" aria-selected="true" aria-controls="{mid}-l">Lines</button>
<button type="button" role="tab" aria-selected="false" aria-controls="{mid}-r">Refs</button>
<button type="button" role="tab" aria-selected="false" aria-controls="{mid}-v">Views</button></div>
<div class="sw-body"><input class="input" type="search" placeholder="Find a line, ref or view">
<div id="{mid}-l">{items(lines)}</div><div id="{mid}-r" hidden>{items(refs)}</div><div id="{mid}-v" hidden>{items(views)}</div></div>
</div></div>'''


FILES = [
    ("folder", ".layr/", "Add probe bench-checkout", "3 days ago", ""),
    ("folder", "src/", "Checkout: keep the saved card token in the overlay", "2 hours ago", ""),
    ("folder", "web/", "Show saved cards on the payment step", "2 hours ago", ""),
    ("folder", "migrations/", "Add the table saved_cards", "1 day ago", ""),
    ("file", ".env", "Rotate the Stripe test key", "5 days ago", "overlay: secrets"),
    ("file", "config/stripe.key", "Rotate the Stripe test key", "5 days ago", "overlay: secrets"),
    ("file", ".layrattributes", "Rebuild Cargo.lock on merge", "2 weeks ago", ""),
    ("file", "Cargo.toml", "Bump tokio to 1.48", "4 days ago", ""),
    ("file", "layr.toml", "Add the Helm chart runnable", "1 week ago", ""),
    ("file", "package-lock.json", "Show saved cards on the payment step", "2 hours ago", "derived"),
    ("file", "README.md", "Explain the local setup", "3 weeks ago", ""),
]

README = '''<div class="md"><h1>shop</h1>
<p>The Acme web shop: a Rust API and a TypeScript storefront.</p>
<h2>Run it</h2>
<pre><code>layr clone layr+https://cloud.acme.dev/shop
cd shop/lines/main
cargo run -p api &amp;
npm --prefix web run dev</code></pre>
<h2>Layout</h2>
<ul><li><code>src/</code>: the API, checkout and tax rules</li><li><code>web/</code>: the storefront</li>
<li><code>migrations/</code>: SQL migrations, applied in order</li></ul>
<p>Secrets live in the overlay <code>secrets</code>. Ask alice for read access.</p></div>'''


def code_page():
    rows = []
    for ic, name, msg, age, mark in FILES:
        m = ""
        if mark == "derived":
            m = " " + grayb("derived")
        elif mark:
            m = f' <a class="badge" href="overlay.html">{e(mark)}</a>'
        rows.append([f'<span class="filecell">{icon(ic)}<a href="#">{e(name)}</a>{m}</span>',
                     f'<a class="muted" href="state.html">{e(msg)}</a>', f'<span class="muted">{age}</span>'])
    rows.append({"attrs": 'class="dim"', "cells": ['<span class="small">2 overlay files hidden from readers without access</span>', "", ""]})
    tree = table([("Name", ""), ("Last state", ""), ("", "num")], rows)
    main = f'''<div class="stack">
<div class="row">{line_switcher()}
<a class="btn ghost" href="lines.html"><span><b>22</b> <span class="muted">lines</span></span></a>
<a class="btn ghost" href="environments.html"><span><b>14</b> <span class="muted">refs</span></span></a>
<a class="btn ghost" href="states.html"><span><b>312</b> <span class="muted">states</span></span></a>
<a class="btn ghost" href="overlays.html"><span><b>4</b> <span class="muted">overlays</span></span></a>
<span class="spacer"></span><button class="btn" type="button">Go to file</button></div>
<div><div class="headrow">{av("bob")}<b>bob</b><span class="msg muted">Merge change 2d9e5c: Bump tokio to 1.48</span>
<span class="badge ok">✓ 4/4</span>{sid("7d3e9a1c40b2")}<span class="muted">2 hours ago</span>
<a href="states.html" class="muted">312 states</a></div>{tree}</div>
<div class="card"><div class="hd">{icon("file")}<b>README.md</b></div>{README}</div>
</div>'''
    side = f'''<aside class="side">
<section><h3>About</h3><p class="note">The Acme web shop: Rust API and TypeScript storefront.</p>
<ul style="margin-top:8px"><li><a href="#">shop.acme.dev</a></li><li class="muted">MIT license</li></ul></section>
<section><h3>Overlays <a href="overlays.html">All</a></h3><ul>
<li>{sid("secrets")}<span class="r">3 files · 3 readers</span></li>
<li>{sid("local-tools")}<span class="r">only you</span></li></ul></section>
<section><h3>Environments <a href="environments.html">All</a></h3><ul>
<li>{sid("env/prod")}<span class="r">→ release/1.4.2 <span class="ok">✓</span></span></li>
<li>{sid("env/staging")}<span class="r">→ a91c4e07 <span class="ok">✓</span></span></li></ul></section>
<section><h3>Copies <a href="settings-copies.html">All</a></h3><ul>
<li>this store<span class="r">updated 4 min ago</span></li>
<li>1 more encrypted copy<span class="r">S3</span></li>
<li>Git mirror<span class="r">GitHub</span></li></ul></section>
<section><h3>Runnables <a href="registry.html">Images</a></h3><ul>
<li>{sid("app")}<span class="r">image</span></li><li>{sid("worker")}<span class="r">image</span></li>
<li>{sid("chart")}<span class="r">Helm chart</span></li></ul></section>
</aside>'''
    return page("shop", f'<div class="cols">{main}{side}</div>', tab="Code")


def lines_page():
    def radar(n, items):
        if not n:
            return '<span class="muted">none</span>'
        lst = "".join(f"<div>{sid(a)}: {sid(b)}</div>" for a, b in items)
        mid = "rd-" + str(zlib.crc32((str(items) + str(n) + str(len(_rd))).encode()))
        _rd.append(mid)
        return (f'<span class="popover"><button class="btn ghost sm warn" type="button" data-menu="{mid}">● overlaps {n} line{"s" if n > 1 else ""}</button>'
                f'<div class="menu" id="{mid}"><div class="muted small">Files other live lines change too</div>{lst}</div></span>')

    def menu(name):
        mid = "lm-" + name
        return (f'<div class="menu-wrap"><button class="iconbtn" type="button" data-menu="{mid}" aria-label="Actions for {name}">{icon("dots")}</button>'
                f'<div class="menu end" id="{mid}"><button type="button">Rename</button><button type="button">Park</button>'
                f'<button type="button">Replay onto main</button><hr><button type="button" class="bad">Delete</button></div></div>')
    H = [("Line", ""), ("Owner", ""), ("Head", ""), ("Ahead / behind main", "num"),
         ("Conflict radar", ""), ("Last activity", "num"), ("", "")]

    def row(cells, tags, attrs=""):
        return {"attrs": f'data-f="{tags}" {attrs}'.strip(), "cells": cells}
    rows = [
        row(["<a href='code.html'>main</a> " + badge("protected"), who("alice"), sid("7d3e9a1c") + ' <span class="muted">Merge change 2d9e5c</span>',
             '<span class="muted">·</span>', radar(0, []), "2 h", ""], "mine"),
        row(["<a href='change.html'>topic-checkout</a>", who("bob"), sid("5b20e1f7") + ' <span class="muted">Support saved cards</span>', "3 ↑ 1 ↓",
             radar(2, [("agent-4", "src/cart.rs"), ("feature-coupons", "src/cart.rs")]), "40 min", menu("topic-checkout")], ""),
        row(["<a href='change.html'>fix-tax-rounding</a>", who("bob"), sid("e8813c2a") + ' <span class="muted">Round EUR tax per line</span>', "2 ↑ 0 ↓",
             radar(0, []), "1 h", menu("fix-tax-rounding")], ""),
        row(["<a href='change-steps.html'>feature-coupons</a>", who("alice"), sid("0c7f55d1") + ' <span class="muted">auto · agent-7</span>', "38 ↑ 0 ↓",
             radar(1, [("topic-checkout", "src/cart.rs")]), "6 min", menu("feature-coupons")], "mine"),
        row(['<span class="chev">▸</span> exp/ <span class="muted">(2 lines)</span>', who("carol"), "", "", "", '<span class="muted">stale</span>', ""],
            "group", 'class="group" data-group="exp"'),
        row(["<a href='code.html'>exp/redis-cache</a>", who("carol"), sid("4a7e01c9") + ' <span class="muted">Try a Redis cart cache</span>', "5 ↑ 31 ↓",
             radar(0, []), '<span class="muted">23 days</span>', menu("exp-redis-cache")], "stale", 'class="child" data-in="exp"'),
        row(["<a href='code.html'>exp/http3</a>", who("carol"), sid("91d2b6e0") + ' <span class="muted">Serve over HTTP/3</span>', "2 ↑ 58 ↓",
             radar(0, []), '<span class="muted">41 days</span>', menu("exp-http3")], "stale", 'class="child" data-in="exp"'),
        row(['<span class="chev">▸</span> agent-1 … agent-16 <span class="muted">(16 lines, made together)</span>',
             who("alice"), '<span class="muted">from main 7d3e9a1c</span>', "", radar(1, [("topic-checkout", "src/cart.rs")]), "now", ""],
            "group", 'class="group" data-group="agents"'),
    ]
    for i, a in enumerate(AGENTS, 1):
        ahead = [4, 0, 2, 7, 1, 0, 0, 3, 5, 0, 1, 2, 0, 6, 1, 0][i - 1]
        rows.append(row([
            f"<a href='code.html'>{a}</a>", who("alice"), sid(f"{(0x3a1 * i * 7919) % 0xffffffff:08x}") + ' <span class="muted">auto</span>',
            f"{ahead} ↑ 0 ↓", radar(2 if i == 4 else 0, [("topic-checkout", "src/cart.rs"), ("feature-coupons", "src/cart.rs")]),
            f"{i % 9 + 1} min", menu(a)], "mine agents", 'class="child" data-in="agents"'))
    new = dialog("dlg-line", "New line", f'''{field("Name", inp("agent", mono=True))}
{field("From", sel(["main", "topic-checkout", "env/prod", "release/1.4.2"], mono=True))}
<label class="field"><span>Count</span><input class="input" type="number" value="16" min="1" style="width:120px">
<span class="note" style="font-weight:400;margin-top:6px">Make several lines at once, for agents. Names get a number: agent-1, agent-2 …</span></label>
{cmd("layr line new agent main --count 16")}''', '<button class="btn" type="button" data-close>Cancel</button><button class="btn primary" type="button" data-close>Create 16 lines</button>')
    body = f'''{pagehead("Lines", '<button class="btn primary" type="button" data-dialog="dlg-line">New line</button>',
               "Each line is a real folder with its own caches. Names with a slash group together, for example exp/.")}
<div class="filters"><div class="seg" data-filter-for="lines"><button type="button" data-filter="" aria-pressed="true">All</button><button type="button" data-filter="mine" aria-pressed="false">Yours</button><button type="button" data-filter="agents" aria-pressed="false">Agents</button><button type="button" data-filter="stale" aria-pressed="false">Stale</button></div>
<input class="input grow" type="search" placeholder="Filter lines"></div>
<div class="note" data-when="stale" hidden style="margin-bottom:12px">Lines with no save for 21 days. Delete the ones you do not need. Their states stay in history. {cmd("layr line delete exp/http3")}</div>
<div id="lines">{table(H, rows, stacked=True)}</div>
{disk_bars()}'''
    return page("Lines", body, tab="Code", dialogs=new)


STATES = [
    ("Oct 23, 2026", [
        ("Merge change 2d9e5c: Bump tokio to 1.48", "bob", "laptop-bob", "2 hours ago", "7d3e9a1c40b2",
         [("ok", "unit"), ("ok", "lint"), ("ok", "e2e"), ("ok", "approved by alice")], [], [
             ("auto · bob · after merge of 2 parents", "7d3e9a1c")]),
        ("Show saved cards on the payment step", "bob", "laptop-bob", "3 hours ago", "41f0b9d2e6a7",
         [("ok", "unit"), ("ok", "lint"), ("warn", "e2e")], [], [
             ("auto · bob · after npm install (derived package-lock.json)", "40e2aa19"),
             ("auto · bob · after save in web/src/pay.tsx", "40a7c3f1")]),
        ("Add the table saved_cards", "alice", "laptop-alice", "6 hours ago", "a91c4e07f3d8",
         [("ok", "unit"), ("ok", "lint"), ("ok", "e2e"), ("ok", "approved by bob")], ["env/staging"], []),
    ]),
    ("Oct 22, 2026", [
        ("Round EUR tax per line, not per order", "bob", "laptop-bob", "yesterday", "e8813c2a9b10",
         [("ok", "unit"), ("bad", "e2e")], [], [("auto · agent-3 · after tool call apply_patch src/tax.rs", "e87d0c55")]),
        ("Release 1.4.2", "alice", "laptop-alice", "yesterday", "c0ffee4a1b2c",
         [("ok", "unit"), ("ok", "lint"), ("ok", "e2e"), ("ok", "build-image"), ("ok", "approved by bob")],
         ["env/prod", "release/1.4.2"], []),
        ("Fix the cart total for empty carts", "carol", "laptop-alice", "yesterday", "9a0c13d7e2f1",
         [("ok", "unit"), ("ok", "lint")], [], []),
    ]),
    ("Oct 20, 2026", [
        ("Release 1.4.1", "alice", "laptop-alice", "3 days ago", "b17a2c90d4e3",
         [("ok", "unit"), ("ok", "e2e"), ("ok", "build-image")], ["release/1.4.1", "pins/audit-2026-10"], []),
        ("Remove the legacy cart API", "bob", "ci-runner-2", "3 days ago", "6e2d81c0a7b9",
         [("ok", "unit"), ("ok", "lint")], ["holds/legal-hold-17"], []),
    ]),
]


def att(kind, name):
    if kind == "ok":
        return okb(name)
    if kind == "bad":
        return badb(name)
    return warnb(name)


def states_page():
    out = []
    for day, items in STATES:
        out.append(f'<div class="day">Saved on {day}</div><div class="list">')
        for msg, a, m, t, s, atts, refs, autos in items:
            out.append(f'''<div class="item"><div class="main"><a class="title" href="state.html">{e(msg)}</a>
<div class="meta">{who(a)}<span>on {sid(m)}</span><span>{t}</span>{"".join(ref(r) for r in refs)}</div>
<div class="row" style="margin-top:6px">{"".join(att(k, n) for k, n in atts)}</div></div>
<div class="side">{sidc(s[:8])}<a class="btn sm" href="code.html">Browse files</a></div></div>''')
            for txt, sid_ in autos:
                out.append(f'<div class="item auto" data-auto hidden><div class="main">{e(txt)}</div><div class="side">{sid(sid_)}</div></div>')
        out.append("</div>")
    body = f'''{pagehead("States", '<button class="btn" type="button" data-dialog="dlg-bisect">Find the state that broke it</button>',
               "Each state is a frozen, signed copy of the whole folder.")}
<div class="filters">{line_switcher()}<span class="spacer"></span>
<label class="toggle"><input type="checkbox" data-show="[data-auto]"> Show automatic states</label></div>
{"".join(out)}'''
    dlg = dialog("dlg-bisect", "Find the state that broke it", f'''<p class="muted">Layr runs the command on exact states between a good and a bad one.</p>
<div style="display:grid;grid-template-columns:1fr 1fr;gap:16px">{field("Good state", inp("9a0c13d7", mono=True))}{field("Bad state", inp("e8813c2a", mono=True))}</div>
{field("Command", inp("cargo test -p checkout", mono=True))}
<label class="toggle"><input type="checkbox" checked> Search automatic states too</label>
<div class="callout bad"><b>First bad: auto state {sid("c41e7a90")} by agent-7</b>
<p class="note" style="margin-top:4px">After tool call <code>apply_patch src/tax.rs</code>. 9 runs, 4 reused from earlier checks.</p></div>
{cmd("layr bisect run --auto -- cargo test -p checkout")}''', '<button class="btn" type="button" data-close>Close</button><button class="btn primary" type="button">Run</button>')
    return page("States", body, tab="Code", dialogs=dlg)


DIFF_TAX = [
    ("h", "", "", "@@ -41,9 +41,12 @@ fn line_tax(line: &Line, rate: Rate) -> Money {"),
    ("", "41", "41", "    let net = line.price * line.qty;"),
    ("d", "42", "", "    let tax = (net * rate).round_half_even();"),
    ("a", "", "42", "    // Round per line, as EU invoices require."),
    ("a", "", "43", "    let tax = (net * rate).round_half_up_cents();"),
    ("", "43", "44", "    Money::new(tax, line.currency)"),
    ("", "44", "45", "}"),
]


def diff_rows(rows, comment=None):
    out = []
    for kind, a, b, text in rows:
        sg = {"a": "+", "d": "−", "h": "", "": " "}[kind]
        if kind == "h":
            out.append(f'<tr class="h"><td colspan="4">{e(text)}</td></tr>')
        else:
            out.append(f'<tr class="{kind}"><td class="ln">{a}</td><td class="ln">{b}</td><td class="sg">{sg}</td><td class="tx">{e(text)}</td></tr>')
    if comment:
        out.append(f'<tr class="cm"><td colspan="4">{comment}</td></tr>')
    return "".join(out)


def split_rows(rows):
    out = []
    for kind, a, b, text in rows:
        if kind == "h":
            out.append(f'<tr class="h"><td colspan="4">{e(text)}</td></tr>')
        elif kind == "d":
            out.append(f'<tr><td class="ln">{a}</td><td class="tx del" style="background:color-mix(in oklch,var(--red-900) 10%,transparent)">{e(text)}</td><td class="ln"></td><td class="tx"></td></tr>')
        elif kind == "a":
            out.append(f'<tr><td class="ln"></td><td class="tx"></td><td class="ln">{b}</td><td class="tx add" style="background:color-mix(in oklch,var(--green-900) 10%,transparent)">{e(text)}</td></tr>')
        else:
            out.append(f'<tr><td class="ln">{a}</td><td class="tx">{e(text)}</td><td class="ln">{b}</td><td class="tx">{e(text)}</td></tr>')
    return "".join(out)


def diff_frame(path, rows, plus, minus, comment=None, viewed=True):
    return f'''<div class="frame"><div class="fh">{e(path)}<span class="add">+{plus}</span><span class="del">−{minus}</span>
<div class="right">{'<label class="toggle small"><input type="checkbox"> Viewed</label>' if viewed else ''}</div></div>
<table class="diff unified-view">{diff_rows(rows, comment)}</table><table class="diff split-view">{split_rows(rows)}</table></div>'''


def derived_frame(path, tool, plus, minus):
    return f'''<div class="frame collapsed"><div class="fh">{e(path)}<span class="add">+{plus}</span><span class="del">−{minus}</span>{grayb("derived")}
<div class="right"><button class="btn sm" type="button">Show diff</button></div></div>
<div class="collapsed-note" style="border-top:1px solid var(--border-subtle)">Derived by <code>{e(tool)}</code>. Rebuilt, not merged.</div></div>'''


def diff_toggle():
    return ('<div class="seg"><button type="button" data-diffmode="unified" aria-pressed="true">Unified</button>'
            '<button type="button" data-diffmode="split" aria-pressed="false">Split</button></div>')


def state_page():
    H = [("Kind", ""), ("Name", ""), ("Result", ""), ("By", ""), ("Command", ""), ("Time", "num")]
    rows = [
        ["check", "unit", badge("passed"), sid("executor-eu1"), sid("cargo test --workspace"), "2 h ago"],
        ["check", "lint", badge("passed"), sid("executor-eu1"), sid("cargo clippy -- -D warnings"), "2 h ago"],
        ["check", "e2e", badge("passed"), sid("executor-eu1"), sid("npm --prefix web run e2e"), "2 h ago"],
        ["approval", "src/payments/", badge("passed", "✓ approved"), who("alice"), '<span class="muted">·</span>', "2 h ago"],
        ["origin", "merge of 2d9e5c", badge("passed", "✓ signed"), sid("laptop-bob"), '<span class="muted">·</span>', "2 h ago"],
        ["quarantine", "dependency audit", warnb("held"), sid("executor-eu1"), sid("cargo audit"), "1 h ago"],
    ]
    body = f'''<div class="stack">
<div><h1>Merge change 2d9e5c: Bump tokio to 1.48</h1>
<div class="row muted" style="margin-top:8px">{who("bob")}<span>on {sid("laptop-bob")}</span><span>Oct 23, 2026, 19:42</span>
<span>parents {sid("41f0b9d2")} {sid("3c9e0a77")}</span></div>
<div class="row muted small" style="margin-top:6px">Signed by laptop-bob (Ed25519) {sid("SHA256:q8Jt…3nVw")}</div></div>
<div class="row">{action('<button class="btn" type="button">Restore folder to this state</button>', "layr restore --source 7d3e9a1c .")}
{action('<button class="btn" type="button">Pick onto line…</button>', "layr pick 7d3e9a1c")}
{action('<button class="btn" type="button">Invert</button>', "layr invert 7d3e9a1c")}
{action('<a class="btn" href="lines.html">New line from here</a>', "layr line new topic 7d3e9a1c")}</div>
<div class="sect"><h2 style="margin-bottom:12px">Attestations</h2>{table(H, rows)}</div>
<div class="sect diffs"><div class="row" style="margin-bottom:12px"><h2>3 files changed</h2><span class="spacer"></span>{diff_toggle()}</div>
{diff_frame("Cargo.toml", [("h","","","@@ -18,7 +18,7 @@ [dependencies]"),("", "18","18",'serde = "1.0"'),("d","19","",'tokio = { version = "1.47", features = ["full"] }'),("a","","19",'tokio = { version = "1.48", features = ["full"] }'),("", "20","20",'tracing = "0.1"')], 1, 1)}
{derived_frame("Cargo.lock", "cargo", 14, 12)}
{diff_frame("src/tax.rs", DIFF_TAX, 2, 1)}
</div></div>'''
    return page("State 7d3e9a1c", body, tab="Code")


def bisect_page():
    H = [("Step", "num"), ("State", ""), ("Kind", ""), ("Result", ""), ("Reused from attestation", "")]
    rows = [
        ["1", sid("9a0c13d7"), "saved", badge("passed"), "yes"],
        ["2", sid("e8813c2a"), "saved", badge("failed"), "yes"],
        ["3", sid("c3b1f0e2"), "auto", badge("passed"), "no"],
        ["4", sid("c41e7a90"), "auto", badge("failed"), "no"],
        ["5", sid("c40d2b11"), "auto", badge("passed"), "yes"],
        ["6", sid("c41a0e3f"), "auto", badge("passed"), "no"],
        ["7", sid("c41c55d0"), "auto", badge("passed"), "yes"],
        ["8", sid("c41d9e02"), "auto", badge("passed"), "no"],
        ["9", sid("c41e7a90"), "auto", badge("failed"), "no"],
    ]
    body = f'''{pagehead("Find the state that broke it", "", "Layr runs the command on exact states and searches between a good and a bad one.")}
<div class="card"><div class="bd stack-s">
<div style="display:grid;grid-template-columns:1fr 1fr;gap:16px">{field("Good state", inp("9a0c13d7", mono=True))}{field("Bad state", inp("e8813c2a", mono=True))}</div>
{field("Command", inp("cargo test -p checkout", mono=True))}
<label class="toggle"><input type="checkbox" checked> Search automatic states too</label>
<div class="row"><button class="btn primary" type="button">Run</button></div>
{cmd("layr bisect run --auto -- cargo test -p checkout")}</div></div>
<div class="callout bad" style="margin-top:24px"><b>First bad: auto state {sid("c41e7a90")} by agent-7</b>
<p class="muted" style="margin-top:4px">After tool call <code>apply_patch src/tax.rs</code> on line {sid("agent-7")}, 14:02. <a href="state.html">Open the state</a> · <a href="change-steps.html">See the step</a></p></div>
<div class="sect" style="margin-top:24px"><div class="row" style="margin-bottom:12px"><h2>Steps</h2><span class="muted">9 runs, 4 reused</span></div>{table(H, rows)}</div>'''
    return page("Bisect", body, tab="Code")


PAGES = {"code.html": code_page, "lines.html": lines_page, "states.html": states_page,
         "state.html": state_page}
