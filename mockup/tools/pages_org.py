from lib import *


def org(title, body, tab, dialogs=""):
    return page(title, body, scope="org", tab=tab, dialogs=dialogs)


def projects_page():
    P = [("shop", "The Acme web shop: Rust API and TypeScript storefront.", "main", 21, 3, "c0ffee4a", "yesterday", "4 min ago"),
         ("payments", "Payment service and Stripe adapters.", "main", 6, 1, "8e01d2b4", "3 days ago", "1 h ago"),
         ("infra", "CloudFormation and Helm values for all environments.", "main", 3, 0, "51ac7e90", "1 week ago", "2 h ago"),
         ("docs-site", "The public docs site.", "main", 2, 0, "·", "never", "1 day ago")]
    items = "".join(f'''<div class="item"><div class="main"><a class="title" href="code.html">{n}</a><p class="note">{d}</p>
<div class="meta"><span>line {sid(dl)}</span><span>{l} lines</span><span>{c} open changes</span><span>{sid("env/prod")} → {sid(s)} {a}</span></div></div>
<div class="side"><span class="muted small">Encrypted copy: {u}</span></div></div>''' for n, d, dl, l, c, s, a, u in P)
    dlg_new = dialog("dlg-proj", "New project", f'''{field("Name", inp("shop", mono=True))}{field("Import a folder from", inp("~/src/shop", mono=True))}
{field("Owner", sel(["alice", "bob"]))}{cmd("layr init shop --from ~/src/shop --owner alice")}''',
                     '<button class="btn" type="button" data-close>Cancel</button><button class="btn primary" type="button" data-close>Create project</button>')
    dlg_desk = dialog("dlg-desk", "Import from Layr Desk", f'''{field("S3 URL of the Desk vault", inp("s3://desk-bucket/project", mono=True))}
<p class="muted">Signatures are checked. Unsigned old states import as unsigned.</p>{cmd("layr clone s3://desk-bucket/project")}''',
                      '<button class="btn" type="button" data-close>Cancel</button><button class="btn primary" type="button" data-close>Import</button>')
    body = f'''{pagehead("acme", '<button class="btn primary" type="button" data-dialog="dlg-proj">New project</button>', "4 projects. Stores and executors run in the AWS account of acme.")}
<div class="filters"><input class="input grow" type="search" placeholder="Find a project">{sel(["Last activity", "Name"])}</div>
<div class="list">{items}</div>
<div class="row" style="margin-top:16px"><button class="btn" type="button" data-dialog="dlg-desk">Import from Layr Desk</button></div>'''
    return org("acme", body, "Projects", dlg_new + dlg_desk)


def people_page():
    H = [("Person", ""), ("Login", ""), ("Role", ""), ("Person key", ""), ("Projects", "num"), ("Last seen", "num")]
    rows = [[who("Alice Chen"), sid("alice"), "owner", sid("age1alc…9t2q"), "4", "now"],
            [who("Bob Novak"), sid("bob"), "maintainer", sid("age1b0b…4m8x"), "3", "20 min"],
            [who("Carol Diaz"), sid("carol"), "reader", sid("age1c4r…0p3n"), "1", "yesterday"],
            [who("Dan Ito"), sid("dan"), "writer", warnb("invited · waiting for key"), "1", '<span class="muted">·</span>']]
    dlg_inv = dialog("dlg-invite", "Invite person", f'''{field("Login", inp("dan", mono=True))}{field("Projects", sel(["shop", "payments", "infra", "docs-site"]))}
{field("Role", sel(["reader", "writer", "maintainer", "owner"]))}<p class="muted">The person makes a person key. Access starts when you grant it.</p>''',
                        '<button class="btn" type="button" data-close>Cancel</button><button class="btn primary" type="button" data-close>Send invite</button>')
    dlg_grant = dialog("dlg-grant", "Grant access to a store", f'''{field("Person", sel(["carol", "dan"]))}{field("Store", sel(["cloud", "vault"], mono=True))}
<p class="muted">Seals the store key to the person key. Layr Cloud never sees the store key.</p>{cmd("layr remote grant cloud --person carol --recipient age1c4r…0p3n")}''',
                          '<button class="btn" type="button" data-close>Cancel</button><button class="btn primary" type="button" data-close>Grant</button>')
    body = f'''{pagehead("People", '<button class="btn" type="button" data-dialog="dlg-grant">Grant access</button><button class="btn primary" type="button" data-dialog="dlg-invite">Invite person</button>')}
{table(H, rows, stacked=True)}
<section class="sect" id="key" style="margin-top:40px"><h2>Your person key</h2><p class="lede">One key opens every project you may read. Layr Cloud never holds it.</p>
<div class="card" style="margin-top:16px"><div class="bd"><dl class="kv"><dt>Recipient</dt><dd>{sidc("age1alc7w3q0…f5e09t2q")}</dd><dt>Opens</dt><dd>4 projects, 6 stores</dd>
<dt>Recovery</dt><dd>24 recovery words, made on Oct 2, 2026</dd></dl>
<div class="row" style="margin-top:16px"><button class="btn" type="button">Show recovery words</button></div>{cmd("layr person keys")}</div></div></section>'''
    return org("People", body, "People", dlg_inv + dlg_grant)


def stores_page():
    H = [("Project", ""), ("Store URL", ""), ("Region", ""), ("Encryption", ""), ("Size", "num"), ("Objects", "num"), ("Last write", "num"), ("GC", "")]
    rows = [[sid("shop"), sid("layr+https://cloud.acme.dev/shop"), "eu-central-1", "age, end to end", "1.8 GiB", "48,210", "4 min", okb("ok")],
            [sid("payments"), sid("layr+https://cloud.acme.dev/payments"), "eu-central-1", "age, end to end", "640 MiB", "12,904", "1 h", okb("ok")],
            [sid("infra"), sid("layr+https://cloud.acme.dev/infra"), "eu-central-1", "age, end to end", "88 MiB", "2,310", "2 h", warnb("runs tonight")],
            [sid("docs-site"), sid("layr+https://cloud.acme.dev/docs-site"), "eu-central-1", "age, end to end", "41 MiB", "1,022", "1 day", okb("ok")]]
    body = f'''{pagehead("Stores", "", "Storage and Layr Cloud see ciphertext only.")}{table(H, rows)}'''
    return org("Stores", body, "Stores")


def machines_page():
    H = [("Machine", ""), ("OS and store", ""), ("Role", ""), ("Signing key", ""), ("Trust", ""), ("Last seen", "num")]
    on = '<span class="ok">● online</span>'
    rows = [[sid("laptop-alice"), "macOS · APFS", "person laptop", sid("SHA256:a1Lc…7Qe0"), okb("trusted"), on],
            [sid("laptop-bob"), "Linux · btrfs", "person laptop", sid("SHA256:q8Jt…3nVw"), okb("trusted"), on],
            [sid("executor-eu1"), "Linux · btrfs", "executor", sid("SHA256:Ex9u…1mPz"), okb("trusted"), on],
            [sid("ci-runner-2"), "Linux · OverlayFS", "CI runner", sid("SHA256:c2Rn…8kWd"), okb("trusted"), '<span class="muted">○ 3 h ago</span>']]
    JH = [("Job", ""), ("Project", ""), ("Target", ""), ("Started", "num")]
    jobs = [[sid("queue"), sid("shop"), "change 8c03d7 v2, e2e", "3 min"], [sid("probe"), sid("shop"), "bench-checkout on f19a3c07", "1 min"],
            [sid("registry"), sid("shop"), "build app:change-4f2a91", "now"]]
    body = f'''{pagehead("Machines", "", "Each machine signs its records with its own key.")}{table(H, rows, stacked=True)}{cmd("layr remote trust laptop-bob")}
<section class="sect" style="margin-top:40px"><h2>Gateway</h2><p class="lede">Machines and people reach stores without AWS credentials.</p>
<div class="card" style="margin-top:16px"><div class="bd"><dl class="kv"><dt>URL</dt><dd class="mono">https://cloud.acme.dev</dd><dt>TLS</dt><dd>certificate valid until Jan 20, 2027</dd>
<dt>Write tokens</dt><dd>3</dd><dt>GC tokens</dt><dd>1</dd></dl></div></div></section>
<section class="sect"><h2>Executors</h2><p class="lede">{sid("executor-eu1")} runs the queue, checks, the registry and probes.</p>
<div style="margin-top:16px">{table(JH, jobs)}</div></section>'''
    return org("Machines", body, "Machines")


def org_settings_page():
    body = f'''{pagehead("Organization settings")}
<div class="card"><div class="bd stack-s">{field("Name", inp("acme", mono=True))}
<dl class="kv"><dt>AWS account</dt><dd class="mono">4127 0915 3381</dd><dt>Region</dt><dd class="mono">eu-central-1</dd>
<dt>Installed with</dt><dd>CloudFormation stack <span class="mono">layr-cloud-acme</span></dd></dl>
{field("Default role for new members", sel(["reader", "writer"]))}<div class="row"><button class="btn primary" type="button">Save settings</button></div></div></div>
<div class="card danger-zone" style="margin-top:32px"><div class="hd"><b class="bad">Danger zone</b></div><div class="bd row">
<div><b>Delete organization</b><p class="note">Removes the gateway and executors. Stores in your AWS account stay.</p></div><span class="spacer"></span>
<button class="btn danger" type="button">Delete organization</button></div></div>'''
    return org("Settings", body, "Settings")


PAGES = {"index.html": projects_page, "people.html": people_page, "stores.html": stores_page,
         "machines.html": machines_page, "org-settings.html": org_settings_page}
