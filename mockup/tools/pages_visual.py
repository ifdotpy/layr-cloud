"""UI diffs: a screenshot probe compares rendered screens of two exact states."""
from lib import *

W, H = 360, 420


def screen(head):
    """A small drawing of the checkout page. head=True adds saved cards and moves the pay button."""
    t = 'fill="var(--text)"'
    t2 = 'fill="var(--text-2)"'
    box = 'fill="var(--surface)" stroke="var(--border)"'
    parts = [
        f'<rect x="0" y="0" width="{W}" height="{H}" fill="var(--page)"/>',
        f'<rect x="0" y="0" width="{W}" height="36" fill="var(--surface)"/>',
        f'<text x="14" y="23" font-size="12" font-weight="600" {t}>Acme</text>',
        f'<text x="{W - 90}" y="23" font-size="11" {t2}>Cart (2)</text>',
        f'<text x="14" y="66" font-size="16" {t}>Checkout</text>',
        f'<rect x="14" y="80" width="{W - 28}" height="64" rx="6" {box}/>',
        f'<text x="26" y="104" font-size="11" {t}>Coffee beans, 1 kg</text>',
        f'<text x="{W - 70}" y="104" font-size="11" {t}>€24.00</text>',
        f'<text x="26" y="128" font-size="11" {t}>Grinder</text>',
        f'<text x="{W - 70}" y="128" font-size="11" {t}>€89.00</text>',
    ]
    y = 160
    if head:
        parts += [
            f'<text x="14" y="{y + 10}" font-size="12" font-weight="600" {t}>Saved cards</text>',
            f'<rect x="14" y="{y + 20}" width="{W - 28}" height="36" rx="6" fill="var(--surface)" stroke="var(--accent)"/>',
            f'<circle cx="32" cy="{y + 38}" r="5" fill="var(--accent)"/>',
            f'<text x="46" y="{y + 42}" font-size="11" {t}>Visa ending 4242</text>',
            f'<rect x="14" y="{y + 62}" width="{W - 28}" height="36" rx="6" {box}/>',
            f'<circle cx="32" cy="{y + 80}" r="5" fill="none" stroke="var(--text-2)"/>',
            f'<text x="46" y="{y + 84}" font-size="11" {t}>Mastercard ending 5100</text>',
        ]
        y += 112
    parts += [
        f'<text x="14" y="{y + 10}" font-size="12" font-weight="600" {t}>{"Or a new card" if head else "Card"}</text>',
        f'<rect x="14" y="{y + 20}" width="{W - 28}" height="30" rx="6" {box}/>',
        f'<text x="24" y="{y + 39}" font-size="10" {t2}>Card number</text>',
        f'<text x="14" y="{y + 78}" font-size="12" {t}>Total</text>',
        f'<text x="{W - 74}" y="{y + 78}" font-size="12" font-weight="600" {t}>€113.00</text>',
        f'<rect x="14" y="{y + 92}" width="{W - 28}" height="34" rx="6" fill="var(--text)"/>',
        f'<text x="{W // 2 - 26}" y="{y + 113}" font-size="12" font-weight="600" fill="var(--page)">Pay €113.00</text>',
    ]
    return f'<svg viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img" aria-label="{"v3" if head else "base"} screen of /checkout">{"".join(parts)}</svg>'


def diff_overlay():
    """Changed regions of /checkout, as the probe reports them."""
    regions = [(10, 166, W - 20, 104, "added: Saved cards"), (10, 270, W - 20, 160, "moved down 112 px")]
    out = []
    for x, y, w, h, label in regions:
        out.append(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="color-mix(in srgb, var(--amber-900) 18%, transparent)" stroke="var(--amber-900)" stroke-width="1.5" stroke-dasharray="4 3"/>')
        out.append(f'<rect x="{x}" y="{y - 14}" width="{len(label) * 6 + 10}" height="14" fill="var(--amber-900)"/>')
        out.append(f'<text x="{x + 5}" y="{y - 3}" font-size="10" fill="var(--page)">{e(label)}</text>')
    return f'<svg class="overlay" viewBox="0 0 {W} {H}" width="{W}" height="{H}" aria-hidden="true">{"".join(out)}</svg>'


def dom_rows():
    from pages_code import diff_rows
    rows = [("h", "", "", "@@ main.checkout @@"),
            ("", "14", "14", '<section class="items">…</section>'),
            ("a", "", "15", '<section class="saved-cards" aria-label="Saved cards">'),
            ("a", "", "16", '  <h2>Saved cards</h2>'),
            ("a", "", "17", '  <label><input type="radio" name="card" checked> Visa ending 4242</label>'),
            ("a", "", "18", '  <label><input type="radio" name="card"> Mastercard ending 5100</label>'),
            ("a", "", "19", '</section>'),
            ("d", "15", "", '<h2>Card</h2>'),
            ("a", "", "20", '<h2>Or a new card</h2>'),
            ("", "16", "21", '<input name="number" placeholder="Card number">'),
            ("", "17", "22", '<button type="submit">Pay €113.00</button>')]
    return diff_rows(rows)


def visual_card(uid="vd"):
    routes = [("/checkout", "desktop 1280", "3.4 % of pixels", True), ("/checkout", "mobile 390", "5.1 % of pixels", True),
              ("/cart", "desktop 1280", "same", False), ("/", "desktop 1280", "same", False)]
    rows = "".join(
        f'<li><a href="#{uid}" class="mono">{e(r)}</a> <span class="muted small">{e(v)}</span>'
        f'<span class="r">{warnb(c, "●") if ch else okb("same")}</span></li>' for r, v, c, ch in routes)
    base, head = screen(False), screen(True)
    modes = [("side", "Side by side"), ("swipe", "Swipe"), ("onion", "Onion skin"), ("diff", "Difference"), ("dom", "Structure")]
    tabs = "".join(f'<button type="button" role="tab" aria-selected="{"true" if i == 0 else "false"}" aria-controls="{uid}-{k}">{n}</button>'
                   for i, (k, n) in enumerate(modes))
    panels = f'''
<div id="{uid}-side" class="vd-side"><figure><figcaption>base {sid("9a0c13d7")}</figcaption>{base}</figure>
<figure><figcaption>v3 {sid("7d3e9a1c")}</figcaption>{head}</figure></div>
<div id="{uid}-swipe" hidden><div class="vd-stack" data-swipe-box>{base}<div class="vd-top" data-swipe-top>{head}</div></div>
<label class="vd-range">base <input type="range" min="0" max="100" value="50" data-swipe> v3</label></div>
<div id="{uid}-onion" hidden><div class="vd-stack">{base}<div class="vd-top" data-onion-top style="opacity:.5">{head}</div></div>
<label class="vd-range">base <input type="range" min="0" max="100" value="50" data-onion> v3</label></div>
<div id="{uid}-diff" hidden><div class="vd-stack"><div style="opacity:.35">{head}</div><div class="vd-top">{diff_overlay()}</div></div>
<p class="note">2 changed regions. Unchanged pixels are dimmed.</p></div>
<div id="{uid}-dom" hidden><p style="margin-bottom:8px"><b>1 section added, 2 text changes</b> <span class="muted small">HTML snapshot, normalized</span></p>
<div class="frame"><table class="diff">{dom_rows()}</table></div></div>'''
    content = f'''<p><span class="warn">●</span> 2 of 4 screens changed. Each screen is a PNG and an HTML snapshot, made by <code>playwright</code> in the try copy of each exact state.</p>
<div class="vd" id="{uid}"><aside class="side vd-list"><section><h3>Screens</h3><ul>{rows}</ul></section></aside>
<div class="vd-main"><div class="row" style="margin-bottom:12px"><b class="mono">/checkout</b><span class="muted small">desktop 1280</span><span class="spacer"></span>
<div class="dtabs vd-modes" role="tablist">{tabs}</div></div>{panels}</div></div>'''
    from pages_change import probe_card
    return probe_card("storefront-screens", "screenshots", "layr probe run 7d3e --probe storefront-screens", content, warnb("changed"))


CSS = '''
/* UI diffs */
.vd { display: grid; grid-template-columns: 220px minmax(0, 1fr); gap: 16px; }
.vd-list ul li { cursor: pointer; }
.vd-side { display: flex; gap: 16px; flex-wrap: wrap; }
.vd figure { margin: 0; }
.vd figcaption { font-size: 12px; color: var(--text-2); margin-bottom: 6px; }
.vd svg { display: block; border: 1px solid var(--border); border-radius: var(--r); max-width: 100%; height: auto; }
.vd-stack { position: relative; width: 360px; max-width: 100%; }
.vd-top { position: absolute; inset: 0; }
.vd-top svg { border-color: transparent; }
.vd-range { display: flex; align-items: center; gap: 8px; margin-top: 8px; font-size: 12px; color: var(--text-2); width: 360px; max-width: 100%; }
.vd-range input { flex: 1; }
.vd-modes { border-bottom: 0; }
@media (max-width: 767px) { .vd { grid-template-columns: minmax(0, 1fr); } }
'''

JS = '''
  // UI diffs: swipe clips the v3 screen, onion skin fades it.
  $$("input[data-swipe]").forEach((r) => {
    const top = r.closest("[id]").querySelector("[data-swipe-top]");
    const set = () => (top.style.clipPath = "inset(0 0 0 " + r.value + "%)");
    r.addEventListener("input", set); set();
  });
  $$("input[data-onion]").forEach((r) => {
    const top = r.closest("[id]").querySelector("[data-onion-top]");
    r.addEventListener("input", () => (top.style.opacity = r.value / 100));
  });
'''
