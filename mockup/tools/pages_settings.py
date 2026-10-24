from lib import *

SNAV = [
    ("Project", [("Access", "settings-access.html"), ("Protection", "settings-protection.html")]),
    ("Content", [("Overlays", "settings-overlays.html"), ("Secrets", "settings-secrets.html"),
                 ("Merge rules", "settings-merge.html"), ("Caches", "settings-caches.html")]),
    ("Automation", [("Checks and probes", "settings-checks.html")]),
    ("Distribution", [("Remotes", "settings-remotes.html"), ("Registry tokens", "settings-tokens.html")]),
]


def spage(name, content, dialogs=""):
    nav = "".join(f'<div class="grp">{g}</div>' + "".join(
        f'<a href="{h}"{CUR if n == name else ""}>{n}</a>' for n, h in items) for g, items in SNAV)
    body = f'<div class="settings"><nav class="snav" aria-label="Settings">{nav}</nav><div>{content}</div></div>'
    return page(f"{name} settings", body, tab="Settings", dialogs=dialogs)


def sect(title, lede, inner):
    return f'<section class="sect"><h2>{title}</h2><p class="lede">{lede}</p>{inner}</section>'


def access_page():
    H = [("Person or machine", ""), ("Role", ""), ("Granted by", ""), ("Until", "num"), ("", "")]
    rows = [
        [who("alice"), "owner", '<span class="muted">·</span>', "always", ""],
        [who("bob"), "maintainer", who("alice"), "always", '<button class="btn sm" type="button">Change</button>'],
        [who("carol"), "reader", who("alice"), "always", '<button class="btn sm" type="button">Change</button>'],
        [sid("executor-eu1"), "writer", who("alice"), "always", '<button class="btn sm" type="button">Change</button>'],
        [who("dan"), "writer", who("bob"), '<span class="warn">1 h 40 min</span>', '<button class="btn sm danger" type="button">Revoke</button>'],
    ]
    OH = [("Path", ""), ("Owners", ""), ("Approvals needed", "num")]
    owners = [[sid("src/payments/"), "alice", "1"], [sid("web/"), "bob", "1"], [sid("migrations/"), "alice, bob", "1"], [sid(".layr/"), "alice", "1"]]
    c = sect("Access", "Roles: owner, maintainer, writer, reader. A role can expire.",
             table(H, rows) + cmd("layr access grant dan writer --until 2h")) + \
        sect("Path owners", "Reviews of a change ask the owners of the paths it changes.", table(OH, owners) + cmd("layr protect main --path src/payments/=alice")) + \
        tokens_page()
    return spage("Access", c)


def protection_page():
    H = [("Line or ref", ""), ("Required checks", ""), ("Approvals", "num"), ("Queue", ""), ("Who may move", ""), ("", "")]
    rows = [
        [sid("main"), "unit, lint, e2e", "2", "required", "maintainers, through the queue", '<button class="btn sm" type="button">Edit</button>'],
        [sid("env/prod"), "unit, e2e", "1", "·", "alice, bob", '<button class="btn sm" type="button">Edit</button>'],
        [sid("env/staging"), "unit", "0", "·", "writers", '<button class="btn sm" type="button">Edit</button>'],
        [sid("release/*"), "build-image", "1", "·", "alice", '<button class="btn sm" type="button">Edit</button>'],
    ]
    c = sect("Protection", "Rules for lines and refs. A protected ref moves only with compare and swap to a state that passes its rules.",
             table(H, rows) + cmd("layr protect env/prod --check unit --approvals 1"))
    return spage("Protection", c)


def overlays_page():
    def ov(name, patterns, readers, peers, epoch, note, managers):
        rl = "".join(f'<li>{r}<span class="r"><button class="btn sm danger" type="button" data-dialog="dlg-rm">Remove</button></span></li>' for r in readers)
        return f'''<div class="card" style="margin-bottom:16px"><div class="hd"><b class="mono">{name}</b><span class="badge">{epoch}</span><span class="spacer"></span><button class="btn sm" type="button">Add reader</button></div>
<div class="bd"><dl class="kv"><dt>Files</dt><dd>{" ".join(sid(p) for p in patterns)}</dd>
<dt>Peers that may hold it</dt><dd>{peers}</dd><dt>Key</dt><dd class="muted">{note}</dd></dl>
<div class="side" style="margin-top:16px"><h3>Readers</h3><ul>{rl}</ul></div>
<div class="side" style="margin-top:8px"><h3>Key managers <a href="#">Add manager</a></h3><ul>{"".join(f'<li>{m}</li>' for m in managers)}</ul>
<p class="note">Only key managers change the key and the readers. Other machines can still push data encrypted to the published key.</p></div></div></div>'''
    c = sect("Overlays", "Private files with their own history, readers and encryption key. They travel with every push. Only readers can open them.",
             ov("secrets", [".env", ".env.prod", "config/stripe.key"], [who("alice") + ' <span class="muted small">person:alice@acme.dev</span>', who("bob") + ' <span class="muted small">person:bob@acme.dev</span>', sid("machine:executor-eu1")],
                f'{sid("laptop-bob")} {sid("executor-eu1")}', "epoch 3",
                "epoch 3, started when dan was removed. Slots for 3 readers in each store. The signed key record is public; only readers can open it.",
                [sid("laptop-alice") + ' <span class="muted small">creator</span>', sid("laptop-bob")]) +
             ov("local-tools", [".tools/"], [who("alice")], '<span class="muted">none: stays on your machines</span>', "epoch 1", "epoch 1. One slot.", [sid("laptop-alice") + ' <span class="muted small">creator</span>']) +
             '<div class="callout warn" style="margin-bottom:12px"><b class="warn">●</b> <span class="muted">local-tools has one key manager. If laptop-alice is lost, the key cannot change. Add a second manager.</span></div>' +
             cmd("layr overlay readers secrets person:alice@acme.dev=age1alc… person:bob@acme.dev=age1b0b… machine:executor-eu1") +
             cmd("layr overlay managers secrets machine:laptop-bob"))
    dlg = dialog("dlg-rm", "Remove reader", '''<p>The next push starts a new key epoch. The removed reader cannot open new data.</p>
<div class="callout warn"><b class="warn">●</b> Files dan already copied stay readable to dan.</div>''' + cmd("layr overlay readers secrets alice bob executor-eu1"),
                 '<button class="btn" type="button" data-close>Cancel</button><button class="btn danger" type="button" data-close>Remove reader</button>')
    return spage("Private files", c + secrets_page(), dlg)


def secrets_page():
    PH = [("Pattern", ""), ("Action", "")]
    paths = [[sid(".env*"), "refuse in shared files"], [sid("*.key"), "refuse in shared files"], [sid("config/*.pem"), "refuse in shared files"]]
    DH = [("Fingerprint", ""), ("Path", ""), ("Approved by", ""), ("State", ""), ("Reason", "")]
    decl = [[sid("hmac:91c0…7a2e"), sid("tests/fixtures/fake.key"), who("alice"), sid("9a0c13d7"), "Test fixture, not a real key"],
            [sid("hmac:0f3d…c118"), sid("docs/example.env"), who("bob"), sid("b17a2c90"), "Example value from the docs"]]
    RH = [("When", "num"), ("Who", ""), ("Operation", ""), ("Path", ""), ("Result", "")]
    ref_ = [["6 min", who("agent-7"), sid("save"), sid("src/coupons/test.rs:14"), badb("refused")],
            ["yesterday", who("bob"), sid("push cloud"), sid("web/.env.local"), badb("refused")]]
    c = sect("Secret detector", "Layr refuses a save or push that copies a secret value into shared files.", table(PH, paths)) + \
        sect("Declassified values", "Values marked as not secret. Only an HMAC fingerprint is stored, never the value.",
             table(DH, decl) + cmd("layr approve --declassify tests/fixtures/fake.key 9a0c13d7")) + \
        sect("Recent refusals", "", table(RH, ref_))
    return c


def merge_page():
    H = [("Pattern", ""), ("Rule", ""), ("Rebuild command", "")]
    rows = [[sid("package-lock.json"), sid("derive=npm"), sid("npm install --package-lock-only")],
            [sid("Cargo.lock"), sid("derive=cargo"), sid("cargo generate-lockfile --offline")],
            [sid("web/dist/**"), sid("derive=build"), sid("npm --prefix web run build")]]
    src = "".join(f'<span class="l">{e(l)}</span>' for l in ["package-lock.json derive=npm", "Cargo.lock derive=cargo", "web/dist/** derive=build"])
    c = sect("Merge rules", "Derived files are rebuilt after a merge, not merged line by line.",
             table(H, rows) + f'<div class="frame" style="margin-top:16px"><div class="fh">.layrattributes</div><pre class="code">{src}</pre></div>')
    return c


def checks_page():
    H = [("Check", ""), ("Command", ""), ("Runner", "")]
    rows = [[sid("unit"), sid("cargo test --workspace"), sid("executor-eu1")],
            [sid("lint"), sid("cargo clippy -- -D warnings"), sid("executor-eu1")],
            [sid("e2e"), sid("npm --prefix web run e2e"), sid("executor-eu1")],
            [sid("build-image"), sid("layr build app"), sid("executor-eu1")]]
    PH = [("Probe", ""), ("Kind", ""), ("Command", ""), ("Queue limit", "")]
    probes = [[sid("api-catalog"), "HTTP answers", sid("curl -s localhost:8080/api/products?page=2"), "·"],
              [sid("bench-checkout"), "metric", sid("cargo bench -p checkout -- --json"), "fail over +30%"],
              [sid("bundle-size"), "file", sid("npm --prefix web run build"), "fail over +10%"],
              [sid("sql-plan"), "file", sid("psql -f probes/plan.sql"), "·"]]
    c = sect("Checks", "A check is an exact command. Its result is signed and bound to one state.",
             table(H, rows) + cmd("layr config check.unit 'cargo test --workspace'")) + \
        sect("Probes", "Probes record behavior: API answers, metrics and output files.", table(PH, probes) + cmd("layr probe run main"))
    return spage("Checks and probes", c)


def remotes_page():
    SH = [("Store", ""), ("URL", ""), ("Recipient pin", ""), ("Last push", "num"), ("States", "num"), ("Size", "num")]
    stores = [[sid("cloud"), sid("layr+https://cloud.acme.dev/shop"), sid("age1q9x…4k7t"), "4 min", "312", "1.8 GiB"],
              [sid("vault"), sid("s3://acme-layr/shop"), sid("age1q9x…4k7t"), "1 h", "309", "1.8 GiB"]]
    PH = [("Peer", ""), ("URL", ""), ("Machine key", ""), ("Last sync", "num")]
    peers = [[sid("laptop-bob"), sid("ssh://laptop-bob/shop"), sid("SHA256:q8Jt…3nVw"), "20 min"]]
    GH = [("Mirror", ""), ("URL", ""), ("Lines", ""), ("Last push", "num")]
    git = [[sid("github"), sid("git@github.com:acme/shop.git"), sid("main"), "2 h"]]
    trust = '''<div class="callout bad"><b class="bad">✕ Store recipient changed</b>
<p class="note" style="margin-top:4px">The store <span class="mono">vault</span> now names a different recipient. Layr refused to push and fetch.</p>
<dl class="kv" style="margin-top:12px"><dt>Pinned</dt><dd class="mono">age1q9x8r…k2v4k7t</dd><dt>Store now says</dt><dd class="mono">age1m3pz0…w8d0e2c</dd></dl>
<div class="row" style="margin-top:12px"><button class="btn danger" type="button">Trust the new recipient</button></div>''' + \
        cmd("layr remote trust-store vault --recipient age1m3pz0…w8d0e2c") + "</div>"
    c = sect("Stores", "Encrypted copies. Data is encrypted with age before it leaves the machine.",
             table(SH, stores) + cmd("layr remote add vault s3://acme-layr/shop")) + \
        sect("Peers", "Other Layr machines. They apply the same signed access rules.", table(PH, peers) + cmd("layr remote add laptop-bob ssh://laptop-bob/shop")) + \
        sect("Git mirror", "Each state becomes a Git commit with the same parents.", table(GH, git) + cmd("layr git push github main")) + \
        sect("Trust a changed store recipient", "Example of the warning when a store recipient differs from the pin.", trust)
    return spage("Remotes", c)


def tokens_page():
    H = [("Token", ""), ("Scope", ""), ("Created", "num"), ("Last used", "num"), ("Expires", "num"), ("", "")]
    rows = [[sid("alice"), "pull", "Oct 2", "4 min", "Nov 2", '<button class="btn sm danger" type="button">Revoke</button>'],
            [sid("k8s-prod"), "pull", "Sep 14", "1 min", "Dec 14", '<button class="btn sm danger" type="button">Revoke</button>'],
            [sid("bob"), "pull", "Oct 20", "yesterday", "Nov 20", '<button class="btn sm danger" type="button">Revoke</button>']]
    c = sect("Registry tokens", "Tokens let Docker, Helm and Kubernetes pull images. They cannot write.",
             table(H, rows) + cmd("layr access token k8s-prod --until 90d") + cmd("layr access token --revoke bob"))
    return c


def caches_page():
    H = [("Line", ""), ("Cache", ""), ("Size", "num"), ("Shared", "")]
    rows = [[sid("main"), sid("target/"), "1.2 GiB", "source"], [sid("main"), sid("web/node_modules/"), "412 MiB", "source"],
            [sid("topic-checkout"), sid("target/"), "128 MiB", "yes, changes only"], [sid("agent-1 … agent-16"), sid("target/"), "16 × 128 MiB", "yes, changes only"]]
    c = sect("Caches", "Regenerable folders. A new line gets them warm, and they never enter a state.",
             table(H, rows) + cmd("layr cache add target/") + cmd("layr cache list"))
    return c


PAGES = {"settings-access.html": access_page, "settings-protection.html": protection_page,
         "settings-overlays.html": overlays_page, "settings-secrets.html": secrets_page,
         "settings-merge.html": merge_page, "settings-checks.html": checks_page,
         "settings-remotes.html": remotes_page, "settings-tokens.html": tokens_page,
         "settings-caches.html": caches_page}


SNAV = [
    ("Project", [("Access", "settings-access.html"), ("Protection", "settings-protection.html"),
                 ("Private files", "settings-overlays.html"), ("Files", "settings-files.html"), ("Remotes", "settings-remotes.html")]),
]


def files_page():
    return spage("Files", merge_page() + caches_page())


PAGES = {"settings-access.html": access_page, "settings-protection.html": protection_page,
         "settings-overlays.html": overlays_page, "settings-files.html": files_page,
         "settings-remotes.html": remotes_page}


def copies_page():
    H = [("Copy", ""), ("Kind", ""), ("Where", ""), ("Last update", "num"), ("By", "")]
    rows = [["<b>This store</b>", "store, main", sid("layr+https://cloud.acme.dev/shop"), "4 min", sid("laptop-alice")],
            ["Encrypted copy", "store", sid("s3://acme-layr/shop"), "1 h", sid("laptop-alice")],
            ["Git mirror", "export of main", sid("git@github.com:acme/shop.git"), "2 h", sid("executor-eu1")]]
    MH = [("Machine", ""), ("Holds", ""), ("Last sync", "num")]
    machines = [[sid("laptop-alice"), "all lines, overlays secrets and local-tools", "4 min"],
                [sid("laptop-bob"), "main, topic-checkout, fix-tax-rounding, overlay secrets", "20 min"],
                [sid("executor-eu1"), "main and lines in the queue, overlay secrets", "3 min"]]
    c = sect("Copies", "Where this project is kept. Built from signed records that machines write when they push or sync.",
             '<div class="callout" style="margin-bottom:16px"><span class="muted">Remotes are set on each machine with</span> <code>layr remote</code><span class="muted">. Each machine has its own list; this page shows the copies they reported.</span></div>'
             + table(H, rows)) + \
        sect("Machines with this project", "Each machine keeps a full copy of the lines it uses. Private overlays only where the machine is a reader.",
             table(MH, machines) + '<p class="note" style="margin-top:8px"><a href="infrastructure.html">All machines</a></p>')
    return spage("Copies", c)


SNAV = [
    ("Project", [("Access", "settings-access.html"), ("Protection", "settings-protection.html"),
                 ("Private files", "settings-overlays.html"), ("Files", "settings-files.html"), ("Copies", "settings-copies.html")]),
]
PAGES = {"settings-access.html": access_page, "settings-protection.html": protection_page,
         "settings-overlays.html": overlays_page, "settings-files.html": files_page,
         "settings-copies.html": copies_page}
