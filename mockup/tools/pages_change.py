from lib import *
from pages_code import diff_frame, derived_frame, diff_toggle, DIFF_TAX

CHANGES = [
    ("open", "Checkout: support saved cards", "4f2a91", "v3", "bob", None, "topic-checkout",
     "✓ 3/4", "1 of 2", "2 behavior changes", 7, "40 min ago", "change.html"),
    ("queue", "Fix tax rounding for EUR", "8c03d7", "v2", "bob", None, "fix-tax-rounding",
     "⟳ e2e", "2 of 2", "no behavior change", 3, "12 min ago", "queue.html"),
    ("open", "Coupons: percent and fixed", "b71e40", "v1", "alice", "agent-7", "feature-coupons",
     "✓ 2/4", "0 of 1", "1 behavior change", 1, "6 min ago", "change-steps.html"),
]
MERGED = [("Bump tokio to 1.48", "2d9e5c", "bob", "2 hours ago"), ("Add the table saved_cards", "61a0f3", "alice", "6 hours ago")]


def changes_page():
    items = []
    for st, title, n, v, a, agent, line, att, appr, beh, com, upd, href in CHANGES:
        mark = '<span class="mark" aria-hidden="true">○</span>' if st == "open" else '<span class="mark warn" aria-hidden="true">●</span>'
        author = f"{e(agent)} for {e(a)}" if agent else e(a)
        items.append(f'''<div class="item">{mark}<div class="main"><a class="title" href="{href}">{e(title)}</a>
<div class="meta"><span class="mono">{n}</span><span>{v}</span>{badge(st)}<span>by {author}</span><span>{sid(line)} → {sid("main")}</span><span>updated {upd}</span></div></div>
<div class="side"><span class="badge {"ok" if att.startswith("✓") else "warn"}">{e(att)}</span><span class="badge">approvals {appr}</span>
<span class="badge{" warn" if "no " not in beh else ""}">{e(beh)}</span><span class="muted small">{com} comments</span></div></div>''')
    body = f'''{pagehead("Changes", '<button class="btn primary" type="button" data-dialog="dlg-change">New change</button>',
               "A change keeps its id through amends and replays. Each version is an exact state.")}
<div class="filters"><input class="input grow" type="search" value="is:open" style="font-family:var(--mono);font-size:13px">
{sel(["Author"])}{sel(["Line"])}{sel(["Attestation"])}{sel(["Queue"])}</div>
<div class="list"><div class="lhd"><b>3 open</b><a class="muted" href="#">41 merged</a><a class="muted" href="#">6 closed</a></div>{"".join(items)}</div>
<h2 style="margin:32px 0 12px">Recently merged</h2>
<div class="list">{"".join(f'<div class="item"><span class="mark ok" aria-hidden="true">✓</span><div class="main"><a class="title" href="state.html">{e(t)}</a><div class="meta"><span class="mono">{n}</span>{badge("merged")}<span>by {a}</span><span>{w}</span></div></div></div>' for t, n, a, w in MERGED)}</div>'''
    dlg = dialog("dlg-change", "New change", f'''<p class="muted">Every save on a line already belongs to a change. Pick the line to ask for review and merge.</p>
{field("Line", sel(["topic-checkout", "feature-coupons", "fix-tax-rounding"], mono=True))}
{field("Into", sel(["main"], mono=True))}{field("Title", inp("Checkout: support saved cards"))}
{field("Description", '<textarea class="input" rows="4">Saved cards are stored as Stripe tokens in the overlay secrets.</textarea>')}''', '<button class="btn" type="button" data-close>Cancel</button><button class="btn primary" type="button" data-close>Ask for review</button>')
    return page("Changes", body, tab="Changes", dialogs=dlg)


# ---------- change detail ----------

C812 = dict(n="4f2a91", title="Checkout: support saved cards", author="bob", line="topic-checkout", v="v3", agent=None)
C820 = dict(n="b71e40", title="Coupons: percent and fixed", author="alice", line="feature-coupons", v="v1", agent="agent-7")

CTABS = [("Overview", "change.html", None), ("Files", "change-files.html", "4"), ("Behavior", "change-behavior.html", "2"),
         ("Checks", "change-checks.html", "4"), ("Steps", "change-steps.html", "38")]


def chead(c, tab):
    who_ = f"{e(c['agent'])} for {e(c['author'])}" if c["agent"] else e(c["author"])
    vers = ["v1 · Oct 22, 10:14", "v2 · Oct 22, 16:40", "v3 · Oct 23, 18:55"] if c["v"] == "v3" else ["v1 · Oct 23, 21:20"]
    vm = "".join(f'<a href="change.html">{e(x)}</a>' for x in vers)
    tabs = "".join(f'<a href="{h}"{CUR if n == tab else ""}>{n}{f"<span class=count>{k}</span>" if k else ""}</a>' for n, h, k in CTABS)
    return f'''<div class="chead"><h1>{e(c["title"])} <span class="id mono" style="font-size:20px">{c["n"]}</span></h1>
<div class="meta">{badge("open")}<span><b style="color:var(--text);font-weight:500">{who_}</b> wants to merge {sid(c["line"])} into {sid("main")}</span>
<div class="menu-wrap"><button class="btn sm" type="button" data-menu="m-ver">{c["v"]} {icon("chev")}</button><div class="menu" id="m-ver"><div class="lbl">Versions</div>{vm}</div></div></div></div>
<div class="tabs-wrap" style="margin:16px 0 24px"><nav class="tabs" style="padding:0" aria-label="Change">{tabs}</nav></div>'''


def cside(c):
    secrets = (f'''<section><h3>Secrets</h3><div class="callout warn"><b class="warn">● Possible secret</b>
<p class="note" style="margin-top:4px">in {sid("src/coupons/test.rs:14")}: a value that looks like a Stripe key.</p>
<div class="row" style="margin-top:8px"><button class="btn sm" type="button">Move to overlay secrets</button><button class="btn sm" type="button">Mark as not a secret</button></div>
{cmd("layr approve --declassify src/coupons/test.rs@a1f3 0c7f55d1")}</div></section>''' if c["n"] == "b71e40"
               else '<section><h3>Secrets</h3><p class="note"><span class="ok">✓</span> No secret values in shared files</p></section>')
    return f'''<aside class="side">
<section><h3>Reviewers</h3><ul>
<li>{sid("src/payments/")}<span class="r">alice <span class="ok">✓ approved v3</span></span></li>
<li>{sid("web/")}<span class="r">bob <span class="warn">● waiting</span></span></li></ul>
<p class="note" style="margin-top:8px">Approvals bind to an exact state. A new version needs new approvals for changed paths only.</p></section>
<section><h3>Required for main <a href="settings-protection.html">Rules</a></h3><ul>
<li><span class="ok">✓</span> unit</li><li><span class="ok">✓</span> lint</li><li><span class="warn">●</span> e2e <span class="r">pending</span></li>
<li><span class="warn">●</span> approvals <span class="r">1 of 2</span></li></ul></section>
<section><h3>Queue</h3><p class="note" style="margin-bottom:8px">Not in queue</p>
<button class="btn primary" type="button" style="width:100%">Add to queue</button>{cmd("layr queue add " + c["line"])}</section>
<section><h3>Preview</h3><p class="note">Image tag</p>{copyfield("app:change-" + c["n"])}
<p class="note" style="margin-top:8px">Pull</p>{copyfield("docker pull cloud.acme.dev/shop/app:change-" + c["n"])}</section>
<section><h3>Conflict radar</h3><p class="note"><span class="warn">●</span> {sid("src/cart.rs")} also changes in {sid("agent-4")}</p></section>
{secrets}
</aside>'''


def cpage(c, tab, main, file, dialogs=""):
    body = chead(c, tab) + f'<div class="cols"><div class="stack">{main}</div>{cside(c)}</div>'
    return page(f"{c['title']} {c['n']}", body, tab="Changes", dialogs=dialogs)


def overview_page():
    main = f'''<div class="card"><div class="hd">{who("bob")}<span class="muted">opened this change on Oct 22</span></div>
<div class="bd md" style="padding:16px"><p>Customers can save a card at checkout and use it next time.</p>
<ul><li>The Stripe customer token is kept in the overlay <code>secrets</code>, never in shared files.</li>
<li>New table <code>saved_cards</code> with a migration.</li><li>The payment step lists saved cards first.</li></ul></div></div>
<div class="sect"><h2 style="margin-bottom:16px">Versions</h2><ul class="tl">
<li class="major"><b>v3</b> {sid("7d3e9a1c")} <span class="when">· Oct 23, 18:55</span><p class="note">Content: handle expired cards (+24 −6 in 2 files)</p></li>
<li><span class="muted">Replayed onto main {sid("9a0c13d7")}</span> <span class="when">· Oct 23, 09:10</span><p class="note">Base move only. No content change. Hidden in the file compare.</p></li>
<li class="major"><b>v2</b> {sid("5b20e1f7")} <span class="when">· Oct 22, 16:40</span><p class="note">Content: move the token to the overlay secrets (+40 −31 in 4 files)</p></li>
<li class="major"><b>v1</b> {sid("31d8c0aa")} <span class="when">· Oct 22, 10:14</span><p class="note">First version (+212 −18 in 7 files)</p></li></ul></div>
<div class="sect"><h2 style="margin-bottom:16px">Attestations on v3</h2><ul class="tl">
<li>{okb("unit")} <span class="muted">by executor-eu1, reused from v2 for unchanged argv and state</span> <span class="when">· 18:57</span></li>
<li>{okb("lint")} <span class="muted">by executor-eu1</span> <span class="when">· 18:58</span></li>
<li>{okb("approved by alice")} <span class="muted">for src/payments/</span> <span class="when">· 19:20</span></li>
<li>{warnb("e2e", "⟳")} <span class="muted">running on executor-eu1, 3 of 5 min</span></li></ul></div>
<div class="sect"><h2 style="margin-bottom:16px">Conversation</h2>
<div class="thread" style="margin:0"><div class="c"><div class="who">{av("alice")}<b>alice</b><span class="muted">on {sid("src/payments/card.rs:88")} · v2</span></div>
Do we delete the token when the customer removes the card?</div>
<div class="c"><div class="who">{av("bob")}<b>bob</b><span class="muted">· v3</span></div>Yes. v3 calls <code>stripe.detach</code> and removes the row.</div></div>
<textarea class="input" rows="3" placeholder="Leave a comment" style="width:100%;margin-top:12px"></textarea></div>
<div class="card"><div class="hd"><b>Merge into main</b></div><div class="bd stack-s">
<ul style="list-style:none;padding:0;margin:0" class="stack-s"><li><span class="ok">✓</span> unit, lint passed on {sid("7d3e9a1c")}</li>
<li><span class="warn">●</span> e2e is running</li><li><span class="warn">●</span> 1 of 2 approvals: web/ needs bob</li></ul>
<div class="row"><button class="btn" type="button" disabled>Merge</button><button class="btn" type="button" disabled>Merge as one state</button>
<span class="note">Waiting for e2e and 1 approval.</span></div>
{cmd("layr merge topic-checkout --expect 7d3e9a1c")}</div></div>'''
    return cpage(C812, "Overview", main, "change.html")


def files_page():
    tree = "".join(f'<li{" style=\"padding-left:20px\"" if p.startswith("  ") else ""}><span class="filecell">{icon(i)}<a href="#{e(p.strip())}" class="mono">{e(p.strip())}</a></span><span class="r"><span class="add">+{a}</span> <span class="del">−{d}</span></span></li>'
                   for i, p, a, d in [("folder", "src/payments/", 18, 4), ("file", "  card.rs", 14, 3), ("file", "  token.rs", 4, 1),
                                      ("folder", "web/src/", 6, 2), ("file", "  pay.tsx", 6, 2), ("file", "package-lock.json", 41, 12)])
    card = [("h", "", "", "@@ -80,8 +80,14 @@ pub fn remove_card(user: &User, id: CardId) -> Result<()> {"),
            ("", "80", "80", "    let card = cards::find(user, id)?;"),
            ("d", "81", "", "    cards::delete(card.id)?;"),
            ("a", "", "81", "    if card.expired() {"),
            ("a", "", "82", "        cards::delete(card.id)?;"),
            ("a", "", "83", "        return Ok(());"),
            ("a", "", "84", "    }"),
            ("a", "", "85", "    stripe::detach(&card.token)?;"),
            ("a", "", "86", "    cards::delete(card.id)?;"),
            ("", "82", "87", "    Ok(())"),
            ("", "83", "88", "}")]
    thread = f'''<div class="thread"><div class="c"><div class="who">{av("alice")}<b>alice</b><span class="muted">line 85</span></div>
Good. Log the Stripe error code here too.</div><div class="c"><div class="who">{av("bob")}<b>bob</b></div>Done in v3.</div></div>'''
    pay = [("h", "", "", "@@ -12,6 +12,10 @@ export function PaymentStep({ cards }: Props) {"),
           ("", "12", "12", "  return ("),
           ("a", "", "13", "    <SavedCards cards={cards} />"),
           ("", "13", "14", "    <CardForm />")]
    main = f'''<div class="card"><div class="bd row">
<span class="muted">Compare</span>{sel(["v2"])}<span>→</span>{sel(["v3"])}
<label class="toggle"><input type="checkbox" checked> Hide base moves</label><span class="spacer"></span>{diff_toggle()}</div>
<div class="ft">{cmd("layr change diff 4f2a91 --from v2 --to v3")}</div></div>
<div style="display:grid;grid-template-columns:240px minmax(0,1fr);gap:16px" class="files-grid">
<aside class="side"><section><h3>4 files</h3><ul>{tree}</ul></section></aside>
<div class="diffs">{diff_frame("src/payments/card.rs", card, 6, 1, thread)}
{diff_frame("web/src/pay.tsx", pay, 1, 0)}
{derived_frame("package-lock.json", "npm", 41, 12)}</div></div>
<style>@media (max-width:1011px){{.files-grid{{grid-template-columns:minmax(0,1fr)!important}}}}</style>'''
    return cpage(C812, "Files", main, "change-files.html")


def probe_card(name, kind, command, content, note=""):
    return f'''<div class="card"><div class="hd"><b class="mono">{e(name)}</b><span class="badge">{e(kind)}</span><span class="spacer"></span>{note}</div>
<div class="bd stack-s">{content}<p class="note">{sid(command)} · Ran on executor-eu1, exact states</p></div></div>'''


def behavior_cards():
    json_diff = [("", "1", "1", "{"), ("", "2", "2", '  "page": 2,'),
                 ("d", "3", "", '  "items": 24,'), ("a", "", "3", '  "items": 25,'),
                 ("d", "4", "", '  "next": null'), ("a", "", "4", '  "next": "/api/products?page=3"'), ("", "5", "5", "}")]
    from pages_code import diff_rows
    H = [("Metric", ""), ("Base", "num"), ("v3", "num"), ("Change", "num")]
    return "".join([
        probe_card("api-catalog", "HTTP answers", "layr probe run 7d3e --probe api-catalog",
                   f'<p><span class="mono">GET /api/products?page=2</span>: 200 → 200, <span class="warn">● body changed</span> (2 fields)</p>'
                   f'<div class="frame"><table class="diff">{diff_rows(json_diff)}</table></div>', warnb("changed")),
        probe_card("bench-checkout", "metric", "layr probe run 7d3e --probe bench-checkout",
                   table(H, [["p95 latency", "41 ms", "52 ms", '<span class="warn">▲ +27%</span>'],
                             ["throughput", "1,920 req/s", "1,905 req/s", '<span class="muted">−0.8%</span>']]), warnb("changed")),
        probe_card("bundle-size", "file", "layr probe run 7d3e --probe bundle-size",
                   f'<p>{sid("web/dist/app.js")} 214 KiB → 221 KiB <span class="muted">(+3%)</span></p>', grayb("within limit")),
        probe_card("sql-plan", "file", "layr probe run 7d3e --probe sql-plan", '<p><span class="ok">✓</span> No change</p>', okb("same")),
    ])


def behavior_page():
    main = f'''<div class="row"><h2>Behavior diff: base {sid("9a0c13d7")} → v3 {sid("7d3e9a1c")}</h2><span class="spacer"></span>
<div><button class="btn" type="button">Rerun probes</button>{cmd("layr probe diff 9a0c 7d3e")}</div></div>
<p class="note">Probes compare what the program does, not only the code: API answers, metrics and output files.</p>
{behavior_cards()}'''
    return cpage(C812, "Behavior", main, "change-behavior.html")


def checks_rows():
    return [
        ["unit", sid("7d3e9a1c"), badge("passed"), "2m 14s", sid("executor-eu1"), sid("cargo test --workspace"), grayb("reused from earlier run")],
        ["lint", sid("7d3e9a1c"), badge("passed"), "41s", sid("executor-eu1"), sid("cargo clippy -- -D warnings"), ""],
        ["e2e", sid("7d3e9a1c"), badge("running"), "3m 02s", sid("executor-eu1"), sid("npm --prefix web run e2e"), ""],
        ["e2e", sid("5b20e1f7"), badge("failed"), "4m 40s", sid("ci-runner-2"), sid("npm --prefix web run e2e"), '<span class="muted">v2</span>'],
        ["build-image", sid("7d3e9a1c"), badge("passed"), "1m 05s", sid("executor-eu1"), sid("layr build app"), ""],
    ]


CHECK_H = [("Check", ""), ("State", ""), ("Result", ""), ("Duration", "num"), ("Runner", ""), ("Command", ""), ("", "")]

LOG = """$ npm --prefix web run e2e
> web@1.4.2 e2e
> playwright test

Running 48 tests using 4 workers
  ✓  checkout › pays with a new card (2.1s)
  ✕  checkout › pays with a saved card (30.0s)

  Error: Timed out 30000ms waiting for locator('[data-test=saved-card]')
     at tests/checkout.spec.ts:57:22

  1 failed, 47 passed (4m 40s)"""


def change_checks_page():
    log = "".join(f'<span class="l">{e(l)}</span>' for l in LOG.split("\n"))
    main = f'''{table(CHECK_H, checks_rows())}
<p class="note">A check runs once per exact state and command. The same command on the same state reuses the signed result.</p>
<div class="frame"><div class="fh">e2e on {sid("5b20e1f7")} (v2) {badge("failed")}<div class="right"><button class="btn sm" type="button">Rerun</button></div></div>
<pre class="code">{log}</pre></div>'''
    return cpage(C812, "Checks", main, "change-checks.html")


def change_steps_page():
    steps = [
        ("14:01", "ran", "cargo test -p coupons", "exit 0", 0, 0, "c3b1f0e2"),
        ("14:02", "apply_patch", "src/coupons/mod.rs", "", 34, 2, "c40d2b11"),
        ("14:02", "apply_patch", "src/tax.rs", "", 3, 1, "c41e7a90"),
        ("14:03", "ran", "cargo test -p checkout", "exit 101", 0, 0, "c41e7a90"),
        ("14:05", "apply_patch", "src/tax.rs", "", 2, 3, "c4f00a12"),
        ("14:06", "ran", "cargo test -p checkout", "exit 0", 0, 0, "c4f00a12"),
        ("14:09", "write_file", "src/coupons/test.rs", "", 61, 0, "c52e8d31"),
        ("14:14", "ran", "npm --prefix web run build", "exit 0", 0, 0, "c5a9013b"),
    ]
    items = []
    for t, kind, arg, res, a, d, s in steps:
        r = ""
        if res:
            r = f' → <span class="{"bad" if res != "exit 0" else "ok"}">{e(res)}</span>'
        ch = f'<span class="add">+{a}</span> <span class="del">−{d}</span> · <a href="state.html">View diff</a> · ' if a or d else ""
        items.append(f'''<li class="{"major" if kind != "ran" else ""}"><span class="when">{t}</span> · agent-7 {"ran" if kind == "ran" else kind} <code>{e(arg)}</code>{r}
<p class="note">{ch}{sid(s)} · <a href="#">Restore line to this step</a></p></li>''')
    main = f'''<div class="callout"><b>38 steps by agent-7 in 14 min.</b> <span class="muted">Every step is a state you can open or restore.</span></div>
<ul class="tl">{"".join(items)}<li><span class="muted">30 more steps</span> <a href="#">Show all</a></li></ul>
{cmd("layr change log b71e40")}'''
    return cpage(C820, "Steps", main, "change-steps.html")


PAGES = {"changes.html": changes_page, "change.html": overview_page, "change-files.html": files_page,
         "change-behavior.html": behavior_page, "change-checks.html": change_checks_page,
         "change-steps.html": change_steps_page}
