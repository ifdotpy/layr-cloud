// Layr Cloud mockup interactions. Every page reads correctly without this file.
(function () {
  "use strict";
  const $$ = (s, r = document) => Array.from(r.querySelectorAll(s));

  // Dropdowns: <button data-menu="id"> opens <div class="menu" id="id">.
  function closeMenus(except) {
    $$(".menu.open").forEach((m) => {
      if (m !== except) {
        m.classList.remove("open");
        const b = document.querySelector('[data-menu="' + m.id + '"]');
        if (b) b.setAttribute("aria-expanded", "false");
      }
    });
  }
  $$("[data-menu]").forEach((b) => {
    b.setAttribute("aria-haspopup", "true");
    b.setAttribute("aria-expanded", "false");
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      const m = document.getElementById(b.dataset.menu);
      const open = !m.classList.contains("open");
      closeMenus(m);
      m.classList.toggle("open", open);
      b.setAttribute("aria-expanded", String(open));
      if (open) {
        const f = m.querySelector("input, a, button");
        if (f) f.focus();
      }
    });
  });
  document.addEventListener("click", (e) => {
    if (!e.target.closest(".menu")) closeMenus();
  });

  // Dialogs: <button data-dialog="id"> opens <dialog id="id">. Native dialog gives Esc and focus trap.
  $$("[data-dialog]").forEach((b) =>
    b.addEventListener("click", () => {
      closeMenus();
      document.getElementById(b.dataset.dialog).showModal();
    })
  );
  $$("[data-close]").forEach((b) => b.addEventListener("click", () => b.closest("dialog").close()));
  $$("dialog").forEach((d) =>
    d.addEventListener("click", (e) => {
      if (e.target === d) d.close();
    })
  );

  // Tabs inside a page: [role=tablist] buttons with aria-controls.
  $$("[role=tablist]").forEach((list) => {
    const tabs = $$("[role=tab]", list);
    tabs.forEach((t) =>
      t.addEventListener("click", (e) => {
        e.stopPropagation();
        tabs.forEach((o) => {
          o.setAttribute("aria-selected", String(o === t));
          document.getElementById(o.getAttribute("aria-controls")).hidden = o !== t;
        });
      })
    );
  });

  // Toggle that shows or hides elements: <input data-show=".selector">.
  $$("input[data-show]").forEach((c) => {
    const apply = () => $$(c.dataset.show).forEach((el) => (el.hidden = !c.checked));
    c.addEventListener("change", apply);
    apply();
  });

  // Collapsible group rows: <tr class="group" data-group="g"> and <tr data-in="g">.
  $$("tr.group").forEach((g) => {
    g.tabIndex = 0;
    const rows = $$('tr[data-in="' + g.dataset.group + '"]');
    const mark = g.querySelector(".chev");
    const set = (open) => {
      rows.forEach((r) => (r.hidden = !open));
      g.setAttribute("aria-expanded", String(open));
      if (mark) mark.textContent = open ? "▾" : "▸";
    };
    set(g.dataset.open === "true");
    const flip = () => set(g.getAttribute("aria-expanded") !== "true");
    g.addEventListener("click", flip);
    g.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") { e.preventDefault(); flip(); }
    });
  });

  // Row filters: <div class="seg" data-filter-for="id"> buttons with data-filter match rows by data-f words.
  $$("[data-filter-for]").forEach((seg) => {
    const box = document.getElementById(seg.dataset.filterFor);
    const btns = $$("[data-filter]", seg);
    btns.forEach((b) =>
      b.addEventListener("click", () => {
        const f = b.dataset.filter;
        btns.forEach((o) => o.setAttribute("aria-pressed", String(o === b)));
        $$("tbody tr", box).forEach((r) => {
          const words = (r.dataset.f || "").split(" ");
          if (!f) {
            if (r.dataset.in) {
              const g = box.querySelector('tr.group[data-group="' + r.dataset.in + '"]');
              r.hidden = g.getAttribute("aria-expanded") !== "true";
            } else r.hidden = false;
          } else r.hidden = !words.includes(f);
        });
        $$("[data-when]").forEach((el) => (el.hidden = el.dataset.when !== f));
      })
    );
  });

  // Copy buttons: <button data-copy="text">.
  $$("[data-copy]").forEach((b) =>
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      if (navigator.clipboard) navigator.clipboard.writeText(b.dataset.copy).catch(() => {});
      const old = b.getAttribute("aria-label");
      b.setAttribute("aria-label", "Copied");
      b.title = "Copied";
      setTimeout(() => { b.setAttribute("aria-label", old || "Copy"); b.title = "Copy"; }, 1200);
    })
  );

  // Unified and split diff views.
  $$("[data-diffmode]").forEach((b) =>
    b.addEventListener("click", () => {
      const split = b.dataset.diffmode === "split";
      $$(".diffs").forEach((d) => d.classList.toggle("split", split));
      $$("[data-diffmode]").forEach((o) => o.setAttribute("aria-pressed", String(o === b)));
    })
  );

  // "/" focuses the search field.
  document.addEventListener("keydown", (e) => {
    if (e.key === "/" && !/INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName)) {
      const s = document.querySelector(".search input");
      if (s) { e.preventDefault(); s.focus(); }
    }
    if (e.key === "Escape") closeMenus();
  });

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

  // "Show CLI commands" in the account menu expands every command hint; the choice is kept.
  const cliOn = () => localStorage.getItem("layr.cli") === "1";
  const applyCli = () => {
    document.documentElement.classList.toggle("cli", cliOn());
    $$("[data-toggle-cli]").forEach((b) => (b.textContent = cliOn() ? "Hide CLI commands" : "Show CLI commands"));
  };
  $$("[data-toggle-cli]").forEach((b) =>
    b.addEventListener("click", () => { localStorage.setItem("layr.cli", cliOn() ? "0" : "1"); applyCli(); })
  );
  applyCli();

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
})();
