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
})();
