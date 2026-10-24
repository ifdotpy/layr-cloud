from lib import *
from pages_info import pulse
from pages_change import CHECK_H, checks_rows, behavior_cards


def queue_page():
    H = [("Position", "num"), ("Change", ""), ("Version", ""), ("Result being tested", ""), ("Checks", ""),
         ("Probes", ""), ("Added by", ""), ("Waiting", "num"), ("", "")]
    acts = '<div class="row" style="flex-wrap:nowrap"><button class="btn sm" type="button">Move up</button><button class="btn sm danger" type="button">Remove</button></div>'
    rows = [
        ["1", '<a href="change.html">Fix tax rounding for EUR</a> ' + sid("8c03d7"), "v2", sid("f19a3c07"),
         warnb("e2e 3/5 min", "⟳"), warnb("bench-checkout", "⟳"), who("bob"), "12 min", acts],
        ["2", '<a href="change.html">Checkout: support saved cards</a> ' + sid("4f2a91"), "v3", '<span class="muted">after 1</span>',
         '<span class="muted">waiting</span>', '<span class="muted">waiting</span>', who("alice"), "2 min", acts],
        {"attrs": 'class="dim"', "cells": ["·", 'Bump tokio to 1.48 ' + sid("2d9e5c"), "v1", sid("7d3e9a1c"), okb("4/4"), okb("same"), who("bob"), "2 h ago", badge("merged")]},
    ]
    body = f'''{subnav([("Open changes", "changes.html"), ("In queue", "queue.html")], "In queue")}{pagehead(f"Queue for {sid('main')}", f'<div>{"<button class=\"btn primary\" type=\"button\">Run queue</button>"}{cmd("layr queue run")}</div>',
               "Each entry merges only after required checks pass on the exact result.")}
{table(H, rows)}
<div class="callout" style="margin-top:16px"><span class="muted">Required:</span> unit, lint, e2e. <span class="muted">Probes:</span> bench-checkout (fail over +30%).
<a href="settings-protection.html">Rules</a> · <a href="queue-empty.html">Empty view</a></div>'''
    return page("Queue", body, tab="Changes")


def queue_empty_page():
    body = f'''{subnav([("Open changes", "changes.html"), ("In queue", "queue.html")], "In queue")}{pagehead(f"Queue for {sid('main')}", "", "Each entry merges only after required checks pass on the exact result.")}
<div class="empty"><h2>The queue is empty</h2><p class="muted">Add a change from its page, or from the command line.</p>{cmd("layr queue add topic-checkout")}</div>'''
    return page("Queue", body, tab="Changes")


def checks_page():
    defs = "".join(f'<li>{sid(n)}<span class="r">{e(r)}</span></li><li class="muted small" style="padding-top:0"><span class="mono">{e(a)}</span></li>'
                   for n, a, r in [("unit", "cargo test --workspace", "executor-eu1"), ("lint", "cargo clippy -- -D warnings", "executor-eu1"),
                                   ("e2e", "npm --prefix web run e2e", "executor-eu1"), ("build-image", "layr build app", "executor-eu1")])
    more = [
        ["unit", sid("e8813c2a"), badge("passed"), "2m 20s", sid("executor-eu1"), sid("cargo test --workspace"), '<span class="muted">fix-tax-rounding</span>'],
        ["e2e", sid("e8813c2a"), badge("failed"), "5m 01s", sid("executor-eu1"), sid("npm --prefix web run e2e"), '<span class="muted">fix-tax-rounding</span>'],
        ["unit", sid("0c7f55d1"), badge("passed"), "2m 09s", sid("executor-eu1"), sid("cargo test --workspace"), '<span class="muted">feature-coupons</span>'],
    ]
    main = f'''{subnav([("Tests", "checks.html"), ("Behavior", "behavior.html")], "Tests")}{pagehead("Checks", "", "Signed check results. Each one names one exact state and one exact command.")}
<div class="filters">{sel(["All checks", "unit", "lint", "e2e", "build-image"])}{sel(["All lines", "main", "topic-checkout"])}{sel(["Any result", "passed", "failed", "running"])}</div>
{table(CHECK_H, checks_rows() + more)}'''
    side = f'''<aside class="side"><section><h3>Check definitions</h3><ul>{defs}</ul>
{cmd("layr config check.unit 'cargo test --workspace'")}</section>
<section><h3>Probe definitions <a href="behavior.html">Results</a></h3><ul>
<li>{sid("storefront-screens")}<span class="r">screenshots</span></li><li>{sid("api-catalog")}<span class="r">HTTP answers</span></li>
<li>{sid("bench-checkout")}<span class="r">metric · fail over +30 %</span></li><li>{sid("bundle-size")}<span class="r">file · fail over +10 %</span></li></ul>
<p class="note" style="margin-top:8px">Defined in <code>.layr/probes.toml</code> of the project.</p></section></aside>'''
    return page("Checks", f'<div class="cols"><div>{main}</div>{side}</div>', tab="Checks")


def behavior_page():
    H = [("Probe", ""), ("Kind", ""), ("Command", ""), ("Last run", "num"), ("Last 10 states", "")]
    rows = [
        [sid("storefront-screens"), "screenshots", sid("playwright screenshot --routes probes/routes.txt"), "2 h ago", '<span class="muted">4 screens, 2 changed in v3</span>'],
        [sid("api-catalog"), "HTTP answers", sid("curl -s localhost:8080/api/products?page=2"), "2 h ago", '<span class="muted">10 same, 1 changed</span>'],
        [sid("bench-checkout"), "metric", sid("cargo bench -p checkout -- --json"), "2 h ago", '<span class="spark">▃▃▄▃▃▄▃▃▃▆</span> <span class="muted">p95 ms</span>'],
        [sid("bundle-size"), "file", sid("npm --prefix web run build"), "2 h ago", '<span class="spark">▂▂▂▃▃▃▃▃▄▄</span> <span class="muted">KiB</span>'],
        [sid("sql-plan"), "file", sid("psql -f probes/plan.sql"), "6 h ago", '<span class="muted">no change</span>'],
    ]
    body = f'''{subnav([("Tests", "checks.html"), ("Behavior", "behavior.html")], "Behavior")}{pagehead("Behavior", "", "Probes record what the program does on an exact state. A behavior diff compares two states.")}
{table(H, rows)}
<div class="card" style="margin-top:24px"><div class="hd"><b>Compare any two states</b></div><div class="bd row" style="align-items:flex-end">
{field("Base", inp("9a0c13d7", mono=True))}{field("Head", inp("7d3e9a1c", mono=True))}<button class="btn primary" type="button">Compare</button></div>
<div class="ft">{cmd("layr probe diff 9a0c 7d3e")}</div></div>
<div class="row" style="margin:24px 0 12px"><h2>Behavior diff: {sid("9a0c13d7")} → {sid("7d3e9a1c")}</h2></div>
<div class="stack">{behavior_cards()}</div>'''
    return page("Behavior", body, tab="Checks")


def environments_page():
    H = [("Ref", ""), ("Current state", ""), ("Release", ""), ("Protection", ""), ("Last moved", ""), ("Image tag", "")]
    rows = [
        ['<a href="environment.html">env/prod</a>', sid("c0ffee4a") + ' <span class="muted">Release 1.4.2</span>', ref("release/1.4.2"),
         '<span class="ok">✓</span> unit · <span class="ok">✓</span> e2e · 1 approval', "alice · yesterday", sid("app:prod")],
        ['<a href="environment.html">env/staging</a>', sid("a91c4e07") + ' <span class="muted">Add the table saved_cards</span>', '<span class="muted">·</span>',
         '<span class="ok">✓</span> unit', "bob · 6 h ago", sid("app:staging")],
    ]
    OH = [("Ref", ""), ("State", ""), ("Note", ""), ("Expires", "num")]
    other = [
        [sid("release/1.4.2"), sid("c0ffee4a"), "Release 1.4.2", "never"],
        [sid("release/1.4.1"), sid("b17a2c90"), "Release 1.4.1", "never"],
        [sid("pins/audit-2026-10"), sid("b17a2c90"), "Kept for the October audit", "never"],
        [sid("holds/legal-hold-17"), sid("6e2d81c0"), "Legal hold: case 17. GC keeps it.", "never"],
        [sid("leases/agent-3"), sid("e87d0c55"), "agent-3 is working on it", '<span class="warn">1 h 52 min</span>'],
    ]
    body = f'''{subnav([("Environments", "environments.html"), ("Images", "registry.html")], "Environments")}{pagehead("Environments", "", "An environment is a ref. It moves only with compare and swap, and keeps its history.")}
{table(H, rows)}
<div class="sect" style="margin-top:40px"><h2 style="margin-bottom:12px">Other refs</h2>{table(OH, other)}
{cmd("layr ref list")}</div>'''
    return page("Environments", body, tab="Deploy")


def environment_page():
    H = [("Position", ""), ("State", ""), ("Moved by", ""), ("Time", "num"), ("Expected value", ""), ("", "")]
    hist = [
        [sid("env/prod@{0}"), sid("c0ffee4a") + " " + ref("release/1.4.2"), who("alice"), "Oct 22, 17:30", sid("b17a2c90"), '<span class="muted">current</span>'],
        [sid("env/prod@{1}"), sid("b17a2c90") + " " + ref("release/1.4.1"), who("alice"), "Oct 20, 11:02", sid("5e0d41a8"), '<button class="btn sm" type="button">Roll back here</button>'],
        [sid("env/prod@{2}"), sid("5e0d41a8"), who("bob"), "Oct 14, 16:45", sid("90ab33c1"), '<button class="btn sm" type="button">Roll back here</button>'],
        [sid("env/prod@{3}"), sid("90ab33c1"), who("alice"), "Oct 9, 10:20", sid("1f7e2d60"), '<button class="btn sm" type="button">Roll back here</button>'],
        [sid("env/prod@{4}"), sid("1f7e2d60"), who("alice"), "Oct 1, 09:12", sid("44c09e1b"), '<button class="btn sm" type="button">Roll back here</button>'],
        [sid("env/prod@{5}"), sid("44c09e1b"), who("bob"), "Sep 24, 15:00", '<span class="muted">none</span>', '<button class="btn sm" type="button">Roll back here</button>'],
    ]
    dlg = dialog("dlg-move", "Move env/prod", f'''{field("Target state", inp("7d3e9a1c", mono=True))}
<div><p style="font-weight:500;font-size:13px;margin-bottom:6px">Required on the target</p><ul style="list-style:none;padding:0;margin:0" class="stack-s">
<li><span class="ok">✓</span> unit passed on 7d3e9a1c</li><li><span class="ok">✓</span> e2e passed on 7d3e9a1c</li><li><span class="warn">●</span> 1 approval: none yet</li></ul></div>
{cmd("layr ref set env/prod 7d3e --expect c0ffee4a")}''', '<button class="btn" type="button" data-close>Cancel</button><button class="btn primary" type="button" disabled>Move</button>')
    body = f'''<div class="pagehead"><div><h1 class="mono" style="font-size:20px">env/prod</h1><p class="lede">Points at {sid("c0ffee4a")} {ref("release/1.4.2")}. Image {sid("cloud.acme.dev/shop/app:prod")}.</p></div>
<div class="right"><div><button class="btn" type="button">Roll back to env/prod@{{1}}</button>{cmd("layr ref set env/prod env/prod@{1}")}</div>
<div><button class="btn primary" type="button" data-dialog="dlg-move">Move env/prod…</button></div></div></div>
<div class="callout" style="margin-bottom:24px"><b>Protection</b> <span class="muted">· required:</span> unit, e2e, 1 approval. <span class="muted">Who may move:</span> alice, bob. <a href="settings-protection.html">Rules</a></div>
<h2 style="margin-bottom:12px">Position history</h2>{table(H, hist)}{cmd("layr ref log env/prod")}'''
    return page("env/prod", body, tab="Deploy", dialogs=dlg)


TOML = """[runnable.app]
kind = "image"
from = "src/"
build = "cargo build --release -p api"
run = ["/app/api"]
port = 8080

[runnable.worker]
kind = "image"
build = "cargo build --release -p worker"
run = ["/app/worker"]

[runnable.chart]
kind = "helm"
path = "deploy/chart\""""


def registry_page():
    H = [("Image", ""), ("Tag", ""), ("Maps to", ""), ("Digest", ""), ("Size", "num")]
    rows = [
        [sid("app"), sid("prod"), ref("env/prod"), sid("sha256:4e1a…90c2"), "38.2 MiB"],
        [sid("app"), sid("1.4.2"), ref("release/1.4.2"), sid("sha256:4e1a…90c2"), "38.2 MiB"],
        [sid("app"), sid("staging"), ref("env/staging"), sid("sha256:b02f…1d7e"), "38.4 MiB"],
        [sid("app"), sid("change-4f2a91"), '<a href="change.html">change 4f2a91</a>', sid("sha256:77c3…e5a0"), "38.6 MiB"],
        [sid("app"), sid("line-topic-checkout"), ref("topic-checkout"), sid("sha256:77c3…e5a0"), "38.6 MiB"],
        [sid("app"), sid("view-9f2c41"), ref("views/9f2c41"), sid("sha256:d9e8…0b14"), "38.3 MiB"],
        [sid("worker"), sid("prod"), ref("env/prod"), sid("sha256:12bd…77f9"), "21.0 MiB"],
    ]
    CH = [("Chart", ""), ("Version", ""), ("Maps to", ""), ("Digest", "")]
    charts = [[sid("chart"), sid("1.4.2"), ref("release/1.4.2"), sid("sha256:0a5c…e811")],
              [sid("chart"), sid("1.4.1"), ref("release/1.4.1"), sid("sha256:93f0…2c6d")]]
    toml = "".join(f'<span class="l">{e(l)}</span>' for l in TOML.split("\n"))
    main = f'''{subnav([("Environments", "environments.html"), ("Images", "registry.html")], "Images")}{pagehead("Images", "", "Every ref, line and change is an image. Same state, same digest.")}
<div class="callout" style="margin-bottom:24px">Images are built on demand from the exact state when a tag is first pulled.</div>
<h2 style="margin-bottom:12px">Images</h2>{table(H, rows)}
<h2 style="margin:32px 0 12px">Helm charts</h2>{table(CH, charts)}
<h2 style="margin:32px 0 12px">Runnables</h2><div class="frame"><div class="fh">layr.toml</div><pre class="code">{toml}</pre></div>'''
    side = f'''<aside class="side"><section><h3>Pull</h3><div class="stack-s">
{copyfield("docker login cloud.acme.dev -u alice --password-stdin")}
{copyfield("docker pull cloud.acme.dev/shop/app:prod")}
{copyfield("helm install shop oci://cloud.acme.dev/shop/chart --version 1.4.2")}</div>
<p class="note" style="margin-top:8px">Get a token with {sid("layr access token alice")}. <a href="settings-access.html">Tokens</a></p></section>
<section><h3>Deterministic</h3><p class="note">Image layers come from the state's files only, so the same state gives the same digest on every machine.</p></section></aside>'''
    return page("Registry", f'<div class="cols"><div>{main}</div>{side}</div>', tab="Deploy")


def activity_page():
    H = [("Operation", ""), ("Who", ""), ("Machine", ""), ("Target", ""), ("Time", "num"), ("Signature", ""), ("", "")]
    undo = lambda op: f'<div><button class="btn sm" type="button">Undo</button></div>'
    rows = [
        [sid("save"), who("agent-7") + ' <span class="muted small">for alice</span>', sid("executor-eu1"), sid("feature-coupons → c5a9013b"), "6 min", okb("signed"), undo("")],
        [sid("merge"), who("bob"), sid("laptop-bob"), sid("main → 7d3e9a1c"), "2 h", okb("signed"), undo("")],
        [sid("push"), who("alice"), sid("laptop-alice"), sid("cloud: 3 states, 1 overlay"), "4 min", okb("signed"), ""],
        [sid("ref set"), who("alice"), sid("laptop-alice"), sid("env/prod → c0ffee4a"), "yesterday", okb("signed"), undo("")],
        [sid("line new"), who("alice"), sid("laptop-alice"), sid("agent-1 … agent-16"), "yesterday", okb("signed"), undo("")],
        [sid("overlay readers"), who("alice"), sid("laptop-alice"), sid("secrets: −dan"), "2 days", okb("signed"), undo("")],
        [sid("undo"), who("bob"), sid("laptop-bob"), sid("restore of src/ on topic-checkout"), "2 days", okb("signed"), ""],
        [sid("attest"), who("executor-eu1"), sid("executor-eu1"), sid("check unit on 7d3e9a1c"), "2 h", okb("signed"), ""],
    ]
    body = f'''{pagehead("Activity", "", "The signed operation log. Every operation names who, where and what. Most can be undone.")}
<div class="filters">{sel(["Anyone", "alice", "bob", "carol"])}{sel(["People and agents", "Agents only"])}{sel(["All machines", "laptop-alice", "laptop-bob", "executor-eu1", "ci-runner-2"])}{sel(["All kinds", "save", "merge", "ref set", "push", "undo"])}</div>
{pulse()}<h2 style="margin:32px 0 12px">Operations</h2>{table(H, rows)}{cmd("layr op log")}<div class="cmd"><span class="t">layr undo</span>{copybtn("layr undo")}</div>'''
    return page("Activity", body, tab="Activity")


PAGES = {"queue.html": queue_page, "queue-empty.html": queue_empty_page, "checks.html": checks_page,
         "behavior.html": behavior_page, "environments.html": environments_page, "environment.html": environment_page,
         "registry.html": registry_page, "activity.html": activity_page}
