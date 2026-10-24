"""Data-driven infographics (spec section 13). Each one reads a data object; the real UI fills it
from layr-model. Motion is off when the user prefers reduced motion."""
import json
from lib import *


def stamp(text, kind="accent"):
    return f'<span class="stamp {kind}">{e(text)}</span>'


# ---------- 1. Activity pulse: who changes the project, and where states go ----------

PULSE = {
    "kpi": [("Saves today", 412), ("Merged today", 9), ("Checks passed", 37)],
    "sources": [("16 agents", 18), ("3 people", 5), ("executor-eu1", 3)],
    "targets": [("env/staging", 3), ("env/prod", 1), ("2 copies", 6)],
}


def pulse():
    w, h, cx, cy = 820, 240, 410, 120
    left = [(150, 50), (150, 120), (150, 190)]
    right = [(670, 50), (670, 120), (670, 190)]
    paths, dots, nodes = [], [], []
    for i, ((label, rate), (x, y)) in enumerate(zip(PULSE["sources"], left)):
        pid = f"pl{i}"
        paths.append(f'<path id="{pid}" d="M{x + 70},{y} C{x + 170},{y} {cx - 130},{cy} {cx - 46},{cy}" class="flow"/>')
        for k in range(min(rate // 3 + 1, 6)):
            dots.append(f'<circle r="3" class="dot" opacity="0"><set attributeName="opacity" to="1" begin="{k * 0.45:.2f}s"/><animateMotion dur="{2.6 - i * 0.3:.1f}s" begin="{k * 0.45:.2f}s" repeatCount="indefinite"><mpath href="#{pid}"/></animateMotion></circle>')
        nodes.append(f'<g><rect x="{x - 70}" y="{y - 15}" width="140" height="30" rx="6" class="node"/><text x="{x}" y="{y + 4}" text-anchor="middle">{e(label)}</text></g>')
    for i, ((label, rate), (x, y)) in enumerate(zip(PULSE["targets"], right)):
        pid = f"pr{i}"
        paths.append(f'<path id="{pid}" d="M{cx + 46},{cy} C{cx + 130},{cy} {x - 170},{y} {x - 70},{y}" class="flow"/>')
        for k in range(min(rate // 2 + 1, 4)):
            dots.append(f'<circle r="3" class="dot out" opacity="0"><set attributeName="opacity" to="1" begin="{k * 0.6:.2f}s"/><animateMotion dur="{2.4 + i * 0.4:.1f}s" begin="{k * 0.6:.2f}s" repeatCount="indefinite"><mpath href="#{pid}"/></animateMotion></circle>')
        nodes.append(f'<g><rect x="{x - 70}" y="{y - 15}" width="140" height="30" rx="6" class="node"/><circle cx="{x - 56}" cy="{y}" r="4" class="ok-dot"/><text x="{x + 6}" y="{y + 4}" text-anchor="middle">{e(label)}</text></g>')
    center = (f'<circle cx="{cx}" cy="{cy}" r="58" class="halo"/><circle cx="{cx}" cy="{cy}" r="44" class="hub"/>'
              f'<text x="{cx}" y="{cy - 2}" text-anchor="middle" class="hub-t">main</text><text x="{cx}" y="{cy + 14}" text-anchor="middle" class="hub-s">via queue</text>')
    svg = f'<svg class="pulse" viewBox="0 0 {w} {h}" role="img" aria-label="Saves flow from agents and people through the queue into main, then to environments and copies">{"".join(paths)}{"".join(dots)}{center}{"".join(nodes)}</svg>'
    kpis = "".join(f'<div><div class="k">{e(k)}</div><div class="v" data-count="{v}">{v}</div></div>' for k, v in PULSE["kpi"])
    feed = [("09:33", "agent-7 saved 38 steps on feature-coupons", "b71e40", stamp("signed")),
            ("09:30", "bob merged change 2d9e5c into main", "7d3e9a1c", stamp("merged", "purple")),
            ("09:27", "env/staging moved to a91c4e07", "a91c4e07", stamp("checked", "green")),
            ("09:24", "laptop-alice pushed to the encrypted copy", "3 states", stamp("encrypted")),
            ("09:21", "executor-eu1 ran e2e on 7d3e9a1c", "4m 40s", "")]
    rows = "".join(f'<div class="feed-row"><span class="muted small mono">{t}</span><span>{e(m)}</span><span class="muted small mono">{e(x)}</span><span class="feed-stamp">{s}</span></div>' for t, m, x, s in feed)
    return f'''<div class="card pulse-card"><div class="hd"><b>acme / shop</b><span class="spacer"></span><span class="badge ok">● live</span></div>
<div class="summary pulse-kpi">{kpis}</div><div class="bd">{svg}</div><div class="feed">{rows}</div></div>'''


# ---------- 2. Merge stepper for a change ----------

def merge_stepper(current=2):
    steps = ["Saved", "Checks", "Approvals", "Queue", "Merged"]
    items = "".join(f'<div class="step{" done" if i < current else ""}{" now" if i == current else ""}"><i></i><span>{s}</span></div>' for i, s in enumerate(steps))
    return f'<div class="stepper" role="list" aria-label="Merge progress">{items}</div>'


# ---------- 3. Disk use toggle on Lines ----------

DISK = {"layr": [("main", 412), ("topic-checkout", 9), ("fix-tax-rounding", 6), ("feature-coupons", 11), ("16 agent lines", 176)],
        "copies": [("main", 412), ("topic-checkout", 405), ("fix-tax-rounding", 398), ("feature-coupons", 420), ("16 agent lines", 6512)]}


def disk_bars():
    mx = max(v for _, v in DISK["copies"])
    rows = []
    for (name, a), (_, b) in zip(DISK["layr"], DISK["copies"]):
        rows.append(f'<div class="bar-row"><span>{e(name)}</span><span class="bar"><i style="width:{a / mx * 100:.2f}%" data-a="{a / mx * 100:.2f}" data-b="{b / mx * 100:.2f}"></i></span>'
                    f'<span class="num" data-a="{a} MiB" data-b="{b} MiB">{a} MiB</span></div>')
    ta, tb = sum(v for _, v in DISK["layr"]), sum(v for _, v in DISK["copies"])
    return f'''<div class="card" style="margin-top:24px"><div class="bd">
<div class="row" style="align-items:flex-end"><div><div class="muted small">Disk use of all 20 lines</div>
<div class="big" data-a="{ta} MiB" data-b="{tb / 1024:.1f} GiB" data-total>{ta} MiB</div></div><span class="spacer"></span>
<div class="seg" data-bars-toggle><button type="button" aria-pressed="true" data-mode="a">Layr</button><button type="button" aria-pressed="false" data-mode="b">As separate copies</button></div></div>
<div class="bars" style="margin-top:16px">{"".join(rows)}</div>
<p class="note" style="margin-top:12px">Measured from the store: a line shares unchanged blocks and caches with the state it came from.</p></div></div>'''


# ---------- 4. Behavior metrics: base and v3 ----------

def metric_bars():
    data = [("p95 latency", "ms", 41, 52, "warn"), ("throughput", "req/s", 1920, 1905, ""), ("bundle size", "KiB", 214, 221, "")]
    rows = []
    for name, unit, a, b, cls in data:
        mx = max(a, b) * 1.15
        rows.append(f'<div class="bar-row"><span>{e(name)}</span><span class="bar {cls}"><i style="width:{a / mx * 100:.2f}%" data-a="{a / mx * 100:.2f}" data-b="{b / mx * 100:.2f}"></i></span>'
                    f'<span class="num" data-a="{a:,} {unit}" data-b="{b:,} {unit}">{a:,} {unit}</span></div>')
    return f'''<div class="row" style="margin-bottom:12px"><span class="muted small">Toggle to see the change</span><span class="spacer"></span>
<div class="seg" data-bars-toggle><button type="button" aria-pressed="true" data-mode="a">base 9a0c13d7</button><button type="button" aria-pressed="false" data-mode="b">v3 7d3e9a1c</button></div></div>
<div class="bars">{"".join(rows)}</div>'''


# ---------- 5. Copy completeness rings ----------

def ring(have, total, label):
    r, c = 34, 2 * 3.14159 * 34
    frac = have / total
    full = have == total
    return f'''<div class="ring-box"><svg class="ring" viewBox="0 0 84 84" role="img" aria-label="{have} of {total} states">
<circle cx="42" cy="42" r="{r}" class="ring-bg"/><circle cx="42" cy="42" r="{r}" class="ring-fg{" full" if full else ""}" stroke-dasharray="{c:.1f}" stroke-dashoffset="{c:.1f}" data-offset="{c * (1 - frac):.1f}"/>
<text x="42" y="42" text-anchor="middle" class="ring-v">{have}</text><text x="42" y="56" text-anchor="middle" class="ring-s">of {total}</text></svg>
<div><b>{e(label)}</b><div style="margin-top:6px">{stamp("complete", "green") if full else f'<span class="muted small">{total - have} states not here yet</span>'}</div></div></div>'''


def rings():
    return f'<div class="rings">{ring(312, 312, "This store")}{ring(309, 312, "Encrypted copy (S3)")}{ring(41, 41, "Git mirror (states of main)")}</div>'


CSS = '''
/* Infographics (spec section 13) */
.stamp { display: inline-block; font-family: var(--mono); font-size: 11px; font-weight: 600; letter-spacing: .08em; text-transform: uppercase;
  padding: 1px 7px; border: 1.5px solid currentColor; border-radius: 4px; transform: rotate(-4deg); color: var(--accent); }
.stamp.green { color: var(--green-900); } .stamp.purple { color: var(--purple-900); }
.pulse-card .pulse-kpi { border: 0; border-radius: 0; border-bottom: 1px solid var(--border-subtle); }
.pulse { width: 100%; max-width: 820px; height: auto; display: block; margin: 0 auto; }
.pulse .flow { fill: none; stroke: var(--border); stroke-width: 1.5; }
.pulse .dot { fill: var(--gray-1000); } .pulse .dot.out { fill: var(--accent); }
.pulse .node { fill: var(--surface); stroke: var(--border); }
.pulse text { font-size: 12px; fill: var(--text); font-family: var(--sans); }
.pulse .halo { fill: color-mix(in srgb, var(--accent) 14%, transparent); }
.pulse .hub { fill: var(--accent); } .pulse .hub-t { fill: #fff; font-weight: 600; font-size: 13px; } .pulse .hub-s { fill: #fff; opacity: .8; font-size: 10px; }
.pulse .ok-dot { fill: var(--green-900); }
.feed { border-top: 1px solid var(--border-subtle); }
.feed-row { display: grid; grid-template-columns: 56px 1fr auto 90px; gap: 12px; align-items: center; padding: 10px 16px; border-bottom: 1px solid var(--border-subtle); font-size: 13px; }
.feed-row:last-child { border-bottom: 0; } .feed-stamp { text-align: right; }
.stepper { display: grid; grid-template-columns: repeat(5, 1fr); gap: 4px; margin-bottom: 14px; }
.stepper .step i { display: block; height: 4px; border-radius: 2px; background: var(--gray-alpha-200); }
.stepper .step.done i { background: var(--green-900); } .stepper .step.now i { background: var(--accent); }
.stepper .step span { display: block; font-size: 11px; color: var(--text-2); margin-top: 4px; } .stepper .step.now span { color: var(--text); font-weight: 600; }
.bars { display: grid; gap: 10px; }
.bar-row { display: grid; grid-template-columns: 170px 1fr 110px; gap: 12px; align-items: center; font-size: 13px; }
.bar-row .bar { width: auto; display: block; height: 8px; border-radius: 4px; background: var(--gray-alpha-200); overflow: hidden; }
.bar-row .bar i { display: block; height: 100%; border-radius: 4px; background: var(--gray-1000); transition: width .6s cubic-bezier(.2,.8,.2,1); }
.bar-row .bar.warn i { background: var(--amber-900); }
.big { font-size: 32px; font-weight: 600; font-variant-numeric: tabular-nums; }
.rings { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 16px; margin: 16px 0 24px; }
.ring-box { display: flex; align-items: center; gap: 14px; border: 1px solid var(--border); border-radius: var(--r); padding: 12px 16px; background: var(--surface); }
.ring { width: 84px; height: 84px; flex: none; }
.ring-bg { fill: none; stroke: var(--gray-alpha-200); stroke-width: 7; }
.ring-fg { fill: none; stroke: var(--accent); stroke-width: 7; stroke-linecap: round; transform: rotate(-90deg); transform-origin: 42px 42px; transition: stroke-dashoffset 1s cubic-bezier(.2,.8,.2,1); }
.ring-fg.full { stroke: var(--green-900); }
.ring-v { font-size: 18px; font-weight: 600; fill: var(--text); font-family: var(--sans); } .ring-s { font-size: 10px; fill: var(--text-2); font-family: var(--sans); }
@media (max-width: 767px) { .bar-row { grid-template-columns: 110px 1fr 90px; } .feed-row { grid-template-columns: 48px 1fr; } .feed-row > :nth-child(3), .feed-row > :nth-child(4) { display: none; } }
@media (prefers-reduced-motion: reduce) { .pulse .dot { display: none; } .bar-row .bar i, .ring-fg { transition: none; } }
'''

JS = '''
  // Infographics: count up, fill rings, toggle bars. Reduced motion shows final values at once.
  const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  $$("[data-count]").forEach((el) => {
    const end = Number(el.dataset.count), t0 = performance.now(), dur = still ? 0 : 900;
    const tick = (t) => { const k = dur ? Math.min(1, (t - t0) / dur) : 1; el.textContent = Math.round(end * (1 - Math.pow(1 - k, 3))).toLocaleString("en-US"); if (k < 1) requestAnimationFrame(tick); };
    requestAnimationFrame(tick);
  });
  requestAnimationFrame(() => $$(".ring-fg").forEach((c) => (c.style.strokeDashoffset = c.dataset.offset)));
  $$("[data-bars-toggle]").forEach((seg) => {
    const box = seg.closest(".card, .bd") || document;
    seg.querySelectorAll("button").forEach((b) => b.addEventListener("click", () => {
      const m = b.dataset.mode;
      seg.querySelectorAll("button").forEach((o) => o.setAttribute("aria-pressed", String(o === b)));
      box.querySelectorAll(".bar i").forEach((i) => (i.style.width = i.dataset[m] + "%"));
      box.querySelectorAll(".num[data-a], [data-total]").forEach((n) => (n.textContent = n.dataset[m]));
    }));
  });
'''
