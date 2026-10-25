"""Deploy > Releases: a release is a ref release/<version> on one exact state (spec section 18)."""
from lib import badge, cmd, dialog, field, inp, page, pagehead, ref, sel, sid, subnav, table, who

DEPLOY_NAV = [("Environments", "environments.html"), ("Releases", "releases.html"), ("Images", "registry.html")]

# version, state, title, by, when, changes since the previous release, environments, checks
RELEASES = [
    ("1.4.2", "c0ffee4a", "Release 1.4.2", "alice", "Oct 22", 4, ["env/prod"], "5 of 5"),
    ("1.4.1", "b17a2c90", "Release 1.4.1", "alice", "Oct 20", 6, [], "3 of 3"),
    ("1.4.0", "5e0d41a8", "Release 1.4.0", "bob", "Oct 14", 11, [], "3 of 3"),
]

CHANGES_142 = [
    ("Round EUR tax per line, not per order", "bob", "e8813c", "fix"),
    ("Fix the cart total for empty carts", "carol", "9a0c13", "fix"),
    ("Show the order number in the receipt mail", "alice", "3b7d20", ""),
    ("Bump tokio to 1.48", "bob", "2d9e5c", ""),
]


def releases_page():
    H = [("Release", ""), ("Changes", "num"), ("Checks", ""), ("By", ""), ("Date", ""), ("Where", "")]
    rows = []
    for i, (v, st, _t, by, when, n, envs, checks) in enumerate(RELEASES):
        tags = " ".join(ref(x) for x in envs)
        latest = ' <span class="badge blue">latest</span>' if i == 0 else ""
        rows.append([
            f'<a href="release.html" class="mono">release/{v}</a>{latest}',
            f'<a href="release.html">{n}</a>',
            f'<span class="ok">✓</span> {checks}',
            who(by), when, tags or '<span class="muted">·</span>',
        ])
    new = dialog("dlg-release", "New release", f'''{field("Version", inp("1.4.3", mono=True))}
{field("From", sel(["main · 7d3e9a1c", "env/staging · a91c4e07"], mono=True))}
<p class="note">A release is a name for one exact state. It never moves. The protection rule for {sid("release/*")} decides who may make one.</p>
{cmd("layr ref set release/1.4.3 main")}''',
                 '<button class="btn" type="button" data-close>Cancel</button><button class="btn primary" type="button" data-close>Create release</button>')
    body = f'''{subnav(DEPLOY_NAV, "Releases")}{pagehead("Releases", '<button class="btn primary" type="button" data-dialog="dlg-release">New release</button>',
              "Each release names one exact state. Its notes are the changes since the release before it.")}
{table(H, rows)}'''
    return page("Releases", body, tab="Deploy", dialogs=new)


def release_page():
    changes = table([("Change", ""), ("By", ""), ("", "")],
                    [[f'<a href="change.html">{t}</a>', who(b), sid(c)] for t, b, c, _k in CHANGES_142])
    checks = " · ".join(f'<span class="ok">✓</span> {c}' for c in ["unit", "lint", "e2e", "build-image"]) + " · approved by bob"
    main = f'''<p class="muted" style="margin-bottom:4px"><a href="releases.html">Releases</a> /</p>
{pagehead('<span class="mono">release/1.4.2</span> <span class="badge blue">latest</span>', "",
          f'alice released state {sid("c0ffee4a")} on Oct 22. In {ref("env/prod")} since Oct 22.')}
<section class="sect"><h2>4 changes since <a class="mono" href="release.html">release/1.4.1</a></h2>
<div style="margin-top:12px">{changes}</div>
<p style="margin-top:12px"><a href="change-files.html">Compare files with release/1.4.1</a></p></section>
<section class="sect" style="margin-top:32px"><h2>Checks on this exact state</h2><p style="margin-top:8px">{checks}</p></section>'''
    side = f'''<aside class="side"><section><h3>Images</h3><ul>
<li>{sid("app:1.4.2")}</li><li>{sid("worker:1.4.2")}</li><li>{sid("chart 1.4.2")}</li></ul>
<p class="note" style="margin-top:8px"><a href="registry.html">All images</a></p></section>
<section><h3>Get it</h3>{cmd("layr line new try-1.4.2 release/1.4.2")}</section></aside>'''
    return page("release/1.4.2", f'<div class="cols"><div>{main}</div>{side}</div>', tab="Deploy")


PAGES = {"releases.html": releases_page, "release.html": release_page}
