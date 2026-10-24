"""Overlays as a first-class place under Code: a list and one page per overlay."""
from lib import *
from pages_info import stamp

OVERLAYS = [
    dict(name="secrets", files=3, readers=["alice", "bob", "machine:executor-eu1"], managers=["laptop-alice (creator)", "laptop-bob"],
         epoch=3, you="reader", changed="5 days ago", peers=["laptop-bob", "executor-eu1"]),
    dict(name="local-tools", files=14, readers=["alice"], managers=["laptop-alice (creator)"],
         epoch=1, you="reader", changed="2 weeks ago", peers=[]),
    dict(name="payroll", files=2, readers=["carol", "machine:ci-runner-2"], managers=["laptop-carol (creator)", "executor-eu1"],
         epoch=2, you="none", changed="1 month ago", peers=["ci-runner-2"]),
]


def reader_chip(r):
    if r.startswith("machine:"):
        return f'<span class="ref">{e(r)}</span>'
    return who(r)


def overlays_list_page():
    rows = []
    for o in OVERLAYS:
        access = okb("you read it") if o["you"] == "reader" else grayb("you cannot read it")
        warn = warnb("1 key manager") if len(o["managers"]) == 1 else ""
        rows.append([f'<a href="overlay.html">{e(o["name"])}</a>', f'{o["files"]} files',
                     '<span class="row" style="gap:6px">' + " ".join(reader_chip(r) for r in o["readers"]) + "</span>",
                     f'{access} {warn}', f'epoch {o["epoch"]}', o["changed"]])
    H = [("Overlay", ""), ("Files", "num"), ("Readers", ""), ("Your access", ""), ("Key", ""), ("Changed", "num")]
    dlg = dialog("dlg-new-overlay", "New overlay", f'''<p class="muted">Private files with their own history, readers and key. They travel encrypted with every push.</p>
{field("Name", inp("secrets", mono=True))}
{field("Readers", inp("alice, bob, machine:executor-eu1", mono=True))}
{field("Files that belong to it", inp(".env*, config/*.key", mono=True))}
<p class="note">This machine becomes the first key manager. Add a second manager soon, so a lost laptop cannot lock the key.</p>
{cmd("layr overlay create secrets --readers alice,bob,machine:executor-eu1")}{cmd("layr overlay route secrets '.env*' 'config/*.key'")}''',
                 '<button class="btn" type="button" data-close>Cancel</button><button class="btn primary" type="button" data-close>Create overlay</button>')
    body = f'''{pagehead("Overlays", '<button class="btn primary" type="button" data-dialog="dlg-new-overlay">New overlay</button>',
          "Private files in this project. Each overlay has its own readers and its own key. Readers see its files inside the line folder; nobody else gets plaintext.")}
{table(H, rows)}
<p class="footnote">You see the names of all overlays. Only readers see their files.</p>'''
    return page("Overlays", body, tab="Code", dialogs=dlg)


def overlay_page():
    o = OVERLAYS[0]
    files = [[sid(".env"), "Rotate the Stripe test key", "1.2 KiB", "5 days ago"],
             [sid(".env.prod"), "Add the production database URL", "0.8 KiB", "2 weeks ago"],
             [sid("config/stripe.key"), "Rotate the Stripe test key", "0.4 KiB", "5 days ago"]]
    readers = [[who("alice"), "person", sid("age1alc…9t2q"), "since Oct 2 (creator)", ""],
               [who("bob"), "person", sid("age1b0b…4m8x"), "added by alice, Oct 3", '<button class="btn sm danger" type="button" data-dialog="dlg-rm-reader">Remove</button>'],
               [sid("machine:executor-eu1"), "machine", sid("SHA256:Ex9u…1mPz"), "added by alice, Oct 10", '<button class="btn sm danger" type="button" data-dialog="dlg-rm-reader">Remove</button>']]
    epochs = f'''<ul class="tl">
<li class="major"><b>epoch 3</b> <span class="when">· Oct 18</span><p class="note">Started when alice removed dan. dan cannot open data saved after this point. 3 slots per store.</p></li>
<li><b>epoch 2</b> <span class="when">· Oct 8</span><p class="note">Started when alice removed machine:ci-runner-2.</p></li>
<li><b>epoch 1</b> <span class="when">· Oct 2</span><p class="note">Created by laptop-alice.</p></li></ul>'''
    history = [["5 days ago", who("alice"), "saved .env, config/stripe.key", stamp("signed")],
               ["Oct 18", who("alice"), "removed reader dan; new key epoch 3", stamp("signed")],
               ["Oct 12", who("alice"), "added key manager laptop-bob", stamp("signed")],
               ["Oct 10", who("alice"), "added reader machine:executor-eu1", stamp("signed")]]
    rm = dialog("dlg-rm-reader", "Remove reader", f'''<p>The next push starts a new key epoch. The removed reader cannot open anything saved after that.</p>
<div class="callout warn"><b class="warn">●</b> Files this reader already copied stay readable to them. Rotate real secrets (passwords, API keys) as well.</div>
{cmd("layr overlay readers secrets alice machine:executor-eu1")}''',
                '<button class="btn" type="button" data-close>Cancel</button><button class="btn danger" type="button" data-close>Remove reader</button>')
    add = dialog("dlg-add-reader", "Add reader", f'''{field("Person or machine", sel(["carol (person)", "dan (person, invited)", "machine:laptop-bob", "machine:ci-runner-2"]))}
<p class="muted">Layr seals the overlay key to the reader's key in each store at the next push. No new epoch is needed.</p>
{cmd("layr overlay readers secrets alice bob machine:executor-eu1 person:carol@acme.dev=age1c4r…0p3n")}''',
                 '<button class="btn" type="button" data-close>Cancel</button><button class="btn primary" type="button" data-close>Add reader</button>')
    move = dialog("dlg-move-files", "Move files into this overlay", f'''{field("Paths", inp("config/sentry.dsn", mono=True))}
<p class="muted">From the next state the files live in the overlay. Earlier states keep them in shared history; Layr does not rewrite history.</p>
{cmd("layr overlay move secrets config/sentry.dsn")}''',
                  '<button class="btn" type="button" data-close>Cancel</button><button class="btn primary" type="button" data-close>Move</button>')
    body = f'''<p class="small" style="margin-bottom:8px"><a href="overlays.html">Overlays</a> <span class="muted">/</span></p>
<div class="pagehead"><div><h1 class="mono" style="font-size:24px">secrets</h1>
<p class="lede">{okb("you read it")} 3 files · 3 readers · 2 key managers · key epoch 3 · {stamp("encrypted")}</p></div>
<div class="right"><button class="btn" type="button" data-dialog="dlg-move-files">Move files in</button></div></div>
<div class="cols"><div class="stack">
<section class="sect"><h2 style="margin-bottom:12px">Files</h2>{table([("Path", ""), ("Last state", ""), ("Size", "num"), ("Changed", "num")], files)}
<p class="note" style="margin-top:8px">Routed here: {sid(".env*")} {sid("config/*.key")}. New files that match go to this overlay.</p></section>
<section class="sect"><div class="row" style="margin-bottom:12px"><h2>Readers</h2><span class="spacer"></span><button class="btn sm" type="button" data-dialog="dlg-add-reader">Add reader</button></div>
{table([("Reader", ""), ("Kind", ""), ("Key", ""), ("Since", ""), ("", "")], readers)}</section>
<section class="sect"><h2 style="margin-bottom:12px">Key epochs</h2>{epochs}</section>
<section class="sect"><h2 style="margin-bottom:12px">History</h2>{table([("When", "num"), ("Who", ""), ("What", ""), ("", "")], history)}{cmd("layr overlay log secrets")}</section>
</div>
<aside class="side">
<section><h3>Key managers <a href="#">Add</a></h3><ul><li>{sid("laptop-alice")}<span class="r">creator</span></li><li>{sid("laptop-bob")}</li></ul>
<p class="note" style="margin-top:8px">Only managers change readers and the key. Other machines can still push data encrypted to it.</p>{cmd("layr overlay managers secrets machine:laptop-bob")}</section>
<section><h3>Peers that may hold it</h3><ul><li>{sid("laptop-bob")}</li><li>{sid("executor-eu1")}</li></ul>
<p class="note" style="margin-top:8px">Other machines get the shared files only.</p></section>
<section><h3>In stores</h3><ul><li>this store<span class="r">epoch 3 · 3 slots</span></li><li>encrypted copy (S3)<span class="r">epoch 3 · 3 slots</span></li></ul></section>
</aside></div>'''
    return page("secrets overlay", body, tab="Code", dialogs=rm + add + move)


PAGES = {"overlays.html": overlays_list_page, "overlay.html": overlay_page}
