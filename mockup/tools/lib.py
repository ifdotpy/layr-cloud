"""Shared page parts for the Layr Cloud mockup. The generated HTML files are the product."""
from html import escape as e

FONTS = ("https://fonts.googleapis.com/css2?family=Geist:wght@400..600"
         "&family=Geist+Mono:wght@400..600&display=swap")

ICONS = {
    "search": '<circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5 14 14"/>',
    "folder": '<path d="M1.75 3.75h4.5l1.5 1.5h6.5v7.5h-12.5z"/>',
    "file": '<path d="M3.75 1.75h5.5l3 3v9.5h-8.5z"/><path d="M9.25 1.75v3h3"/>',
    "chev": '<path d="m4.5 6 3.5 3.5L11.5 6"/>',
    "copy": '<rect x="5.25" y="5.25" width="8.5" height="8.5" rx="1.5"/><path d="M10.75 5.25v-2a1 1 0 0 0-1-1h-6.5a1 1 0 0 0-1 1v6.5a1 1 0 0 0 1 1h2"/>',
    "dots": '<circle cx="3.5" cy="8" r=".9"/><circle cx="8" cy="8" r=".9"/><circle cx="12.5" cy="8" r=".9"/>',
    "plus": '<path d="M8 3v10M3 8h10"/>',
    "close": '<path d="m4 4 8 8M12 4l-8 8"/>',
}


def icon(name, label=None):
    aria = f'role="img" aria-label="{e(label)}"' if label else 'aria-hidden="true"'
    return (f'<svg class="icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" '
            f'stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" {aria}>{ICONS[name]}</svg>')


def copybtn(text):
    return (f'<button class="iconbtn" type="button" data-copy="{e(text)}" aria-label="Copy" '
            f'title="Copy">{icon("copy")}</button>')


def cmd(text):
    """The CLI line under an action (spec 4.6)."""
    return f'<div class="cmd"><span class="t">{e(text)}</span>{copybtn(text)}</div>'


def sid(x):
    return f'<span class="mono">{e(x)}</span>'


def sidc(x):
    return f'<span class="row" style="gap:2px"><span class="mono">{e(x)}</span>{copybtn(x)}</span>'


def ref(x):
    return f'<span class="ref">{e(x)}</span>'


B = {
    "passed": ("ok", "✓ passed"), "failed": ("bad", "✕ failed"), "pending": ("warn", "● pending"),
    "running": ("warn", "⟳ running"), "skipped": ("", "○ skipped"), "merged": ("ok", "merged"),
    "open": ("", "open"), "closed": ("", "closed"), "queue": ("warn", "in queue"),
    "protected": ("", "protected"),
}


def badge(kind, text=None):
    cls, default = B.get(kind, ("", kind))
    return f'<span class="badge {cls}">{e(text) if text else default}</span>'


def okb(text):
    return f'<span class="badge ok">✓ {e(text)}</span>'


def warnb(text, sym="●"):
    return f'<span class="badge warn">{sym} {e(text)}</span>'


def badb(text):
    return f'<span class="badge bad">✕ {e(text)}</span>'


def grayb(text):
    return f'<span class="badge">{e(text)}</span>'


def av(login, sm=True):
    return f'<span class="avatar{" sm" if sm else ""}" aria-hidden="true">{e(login[0].upper())}</span>'


def who(login):
    return f'<span class="row" style="gap:6px">{av(login)}<span>{e(login)}</span></span>'


def table(headers, rows, cls="", stacked=False):
    """headers: list of (label, 'num'|''); rows: list of lists of html or (html, attrs) cells."""
    th = "".join(f'<th class="{c}">{e(h)}</th>' for h, c in headers)
    body = []
    for r in rows:
        tr_attrs = ""
        if isinstance(r, dict):
            tr_attrs, r = r.get("attrs", ""), r["cells"]
        tds = []
        for i, c in enumerate(r):
            hc = headers[i][1] if i < len(headers) else ""
            first = " first" if i == 0 else ""
            label = f' data-l="{e(headers[i][0])}"' if stacked and i > 0 and headers[i][0] else ""
            tds.append(f'<td class="{hc}{first}"{label}>{c}</td>')
        body.append(f"<tr {tr_attrs}>{''.join(tds)}</tr>")
    st = " stacked" if stacked else ""
    return (f'<div class="tbl-frame"><table class="{cls}{st}"><thead><tr>{th}</tr></thead>'
            f'<tbody>{"".join(body)}</tbody></table></div>')


def dialog(id_, title, body, actions):
    return (f'<dialog id="{id_}" aria-labelledby="{id_}-t"><div class="dh"><h2 id="{id_}-t">{e(title)}</h2>'
            f'<button class="iconbtn" type="button" data-close aria-label="Close">{icon("close")}</button></div>'
            f'<div class="db">{body}</div><div class="df">{actions}</div></dialog>')


def field(label, control):
    return f'<label class="field"><span>{e(label)}</span>{control}</label>'


def inp(value="", ph="", mono=False, type_="text"):
    st = ' style="font-family:var(--mono);font-size:13px"' if mono else ""
    return f'<input class="input" type="{type_}" value="{e(value)}" placeholder="{e(ph)}"{st}>'


def sel(options, mono=False):
    st = ' style="font-family:var(--mono);font-size:13px"' if mono else ""
    return f'<select class="input"{st}>' + "".join(f"<option>{e(o)}</option>" for o in options) + "</select>"


def copyfield(text):
    return f'<div class="copyfield"><span>{e(text)}</span>{copybtn(text)}</div>'


# ---------- Shell ----------

PROJECT_TABS = [
    ("Code", "code.html", None), ("Changes", "changes.html", "3"), ("Queue", "queue.html", "1"),
    ("Checks", "checks.html", None), ("Behavior", "behavior.html", None),
    ("Environments", "environments.html", None), ("Registry", "registry.html", None),
    ("Activity", "activity.html", None), ("Settings", "settings-access.html", None),
]
ORG_TABS = [
    ("Projects", "index.html", "4"), ("People", "people.html", "4"), ("Stores", "stores.html", None),
    ("Machines", "machines.html", "4"), ("Settings", "org-settings.html", None),
]


CUR = ' aria-current="page"'


def topbar(scope):
    crumbs = '<a href="index.html">acme</a>'
    if scope == "project":
        crumbs += '<span class="sep">/</span><a href="code.html">shop</a>'
    return f'''<header class="topbar">
<a class="wordmark" href="index.html">Layr</a>
<nav class="crumbs" aria-label="Breadcrumb">{crumbs}</nav>
<label class="search">{icon("search")}<span class="sr">Search</span><input type="search" placeholder="Search or jump to…"><kbd>/</kbd></label>
<div class="right">
<a class="machines-link" href="machines.html">Machines <b>3 online</b></a>
<div class="menu-wrap"><button class="iconbtn" type="button" data-menu="m-plus" aria-label="Create">{icon("plus")}</button>
<div class="menu end" id="m-plus"><a href="index.html">New project</a><a href="lines.html">New line</a><a href="changes.html">New change</a><a href="people.html">Invite person</a></div></div>
<div class="menu-wrap"><button class="iconbtn" type="button" data-menu="m-user" aria-label="Account of alice" style="width:auto">{av("alice", sm=False)}</button>
<div class="menu end" id="m-user"><div class="lbl">Signed in as alice</div><a href="people.html#key">Your person key</a><a href="machines.html">Your machines</a><hr><a href="#" aria-disabled="true">Sign out</a></div></div>
</div>
</header>'''


def clone_dialog():
    body = f'''<div class="dtabs" role="tablist">
<button type="button" role="tab" aria-selected="true" aria-controls="cl-clone">Clone</button>
<button type="button" role="tab" aria-selected="false" aria-controls="cl-mount">Mount</button>
<button type="button" role="tab" aria-selected="false" aria-controls="cl-git">Git</button></div>
<div id="cl-clone" class="stack-s"><p class="muted">Get the project with all lines and states. Data is decrypted on your machine.</p>{copyfield("layr clone layr+https://cloud.acme.dev/shop")}</div>
<div id="cl-mount" class="stack-s" hidden><p class="muted">Opens the project as a folder, read only, decrypted on your machine.</p>{copyfield("layr mount cloud ~/shop")}</div>
<div id="cl-git" class="stack-s" hidden><p class="muted">Layr does not serve Git. Mirror states to a Git host when you need one.</p><p>Mirror to Git:</p>{copyfield("layr git push github main")}</div>'''
    return dialog("dlg-clone", "Clone shop", body, '<button class="btn" type="button" data-close>Close</button>')


def page(title, body, scope="project", tab=None, file=None, dialogs=""):
    head = f'''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{e(title)} · Layr</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="{FONTS.replace("&", "&amp;")}">
<link rel="stylesheet" href="styles.css">
</head>
<body>
{topbar(scope)}
'''
    if scope == "project":
        tabs = "".join(
            f'<a href="{h}"{CUR if n == tab else ""}>{n}'
            f'{f"<span class=count>{c}</span>" if c else ""}</a>' for n, h, c in PROJECT_TABS)
        head += f'''<div class="projhead"><h1><a href="index.html">acme</a> <span class="muted">/</span> <a href="code.html">shop</a></h1>{grayb("protected main")}
<div class="right"><button class="btn" type="button" data-dialog="dlg-clone">Clone</button></div></div>
<div class="tabs-wrap"><nav class="tabs" aria-label="Project">{tabs}</nav></div>
'''
        dialogs += clone_dialog()
    else:
        tabs = "".join(
            f'<a href="{h}"{CUR if n == tab else ""}>{n}'
            f'{f"<span class=count>{c}</span>" if c else ""}</a>' for n, h, c in ORG_TABS)
        head += f'''<div class="projhead"><h1><a href="index.html">acme</a></h1></div>
<div class="tabs-wrap"><nav class="tabs" aria-label="Organization">{tabs}</nav></div>
'''
    return head + f'<main class="page">\n{body}\n</main>\n{dialogs}\n<script src="app.js"></script>\n</body>\n</html>\n'


def pagehead(title, right="", lede=""):
    l = f'<p class="lede">{lede}</p>' if lede else ""
    return f'<div class="pagehead"><div><h1>{title}</h1>{l}</div><div class="right">{right}</div></div>'


def action(btn_html, command):
    return f'<div>{btn_html}{cmd(command)}</div>'
