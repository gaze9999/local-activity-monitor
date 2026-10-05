(function (global) {
  "use strict";
  let serial = 0;
  const tones = new Set(["neutral", "category", "success", "warning", "error"]);
  function element(name, text, className) {
    const node = document.createElement(name);
    if (text != null) node.textContent = String(text);
    if (className) node.className = className;
    return node;
  }
  function button(label, action, className = "") {
    const node = element("button", label, "wb-button " + className);
    node.type = "button";
    if (action) node.addEventListener("click", action);
    return node;
  }
  function tag(label, tone = "neutral") {
    const node = element("span", label, "wb-tag");
    node.dataset.tone = tones.has(tone) ? tone : "neutral";
    return node;
  }
  function setAppearance(root, { mode = "dark", fontSize = 14 } = {}) {
    root.dataset.wbMode = mode === "light" ? "light" : "dark";
    const size = Number(fontSize);
    root.style.setProperty("--wb-font-size", ([12, 14, 16, 18].includes(size) ? size : 14) + "px");
  }
  function bindTabs(nav, entries, { selected = 0, orientation = "horizontal", onSelect } = {}) {
    if (!entries.length) throw new TypeError("Tabs require at least one entry");
    const abort = new AbortController(), id = "wb-tabs-" + ++serial;
    nav.setAttribute("role", "tablist");
    nav.setAttribute("aria-orientation", orientation);
    function select(index, focus = false) {
      if (!Number.isInteger(index) || index < 0 || index >= entries.length) return;
      entries.forEach(({ tab, panel }, i) => {
        tab.setAttribute("aria-selected", String(i === index));
        tab.tabIndex = i === index ? 0 : -1;
        panel.hidden = i !== index;
      });
      selected = index;
      entries[index].panel.scrollTop = 0;
      entries[index].panel.closest(".wb-dialog-body")?.scrollTo(0, 0);
      if (focus) entries[index].tab.focus();
      onSelect?.(index);
    }
    entries.forEach(({ tab, panel }, index) => {
      tab.id ||= id + "-tab-" + index; panel.id ||= id + "-panel-" + index;
      tab.setAttribute("role", "tab"); tab.setAttribute("aria-controls", panel.id);
      panel.setAttribute("role", "tabpanel"); panel.setAttribute("aria-labelledby", tab.id);
      tab.addEventListener("click", () => select(index), { signal: abort.signal });
      tab.addEventListener("keydown", event => {
        const previous = orientation === "vertical" ? "ArrowUp" : "ArrowLeft";
        const next = orientation === "vertical" ? "ArrowDown" : "ArrowRight";
        const target = event.key === previous ? (selected + entries.length - 1) % entries.length :
          event.key === next ? (selected + 1) % entries.length :
          event.key === "Home" ? 0 : event.key === "End" ? entries.length - 1 : null;
        if (target != null) { event.preventDefault(); select(target, true); }
      }, { signal: abort.signal });
    });
    select(Number.isInteger(selected) && selected >= 0 && selected < entries.length ? selected : 0);
    return { select, destroy() { abort.abort(); } };
  }
  function bindDialog(dialog, { beforeClose = () => true } = {}) {
    const abort = new AbortController();
    let opener, frame, outsideDown = false;
    const outside = event => {
      const rect = dialog.getBoundingClientRect();
      return event.target === dialog && (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom);
    };
    function reset() {
      dialog.scrollTop = dialog.scrollLeft = 0;
      dialog.querySelectorAll(".wb-dialog-body, pre, textarea, .wb-table-wrap").forEach(node => { node.scrollTop = node.scrollLeft = 0; });
    }
    function close() {
      if (!dialog.open || beforeClose() === false) return false;
      dialog.close(); return true;
    }
    dialog.addEventListener("pointerdown", event => { outsideDown = outside(event); }, { signal: abort.signal });
    dialog.addEventListener("click", event => { if (outsideDown && outside(event)) close(); outsideDown = false; }, { signal: abort.signal });
    dialog.addEventListener("cancel", event => { event.preventDefault(); close(); }, { signal: abort.signal });
    dialog.addEventListener("close", () => { cancelAnimationFrame(frame); if (opener?.isConnected) opener.focus({ preventScroll: true }); }, { signal: abort.signal });
    return {
      open() {
        if (dialog.open) return;
        opener = document.activeElement; dialog.showModal(); reset();
        dialog.querySelector("[data-wb-close], button, input, select, textarea, [tabindex='0']")?.focus({ preventScroll: true });
        frame = requestAnimationFrame(reset);
      },
      close,
      destroy() { cancelAnimationFrame(frame); if (dialog.open) dialog.close(); abort.abort(); }
    };
  }
  function createToast(root) {
    const region = element("div", null, "wb-toast");
    region.setAttribute("role", "status"); region.setAttribute("aria-live", "polite"); region.setAttribute("aria-atomic", "true");
    root.append(region);
    let timer;
    return {
      show(message, tone = "success", duration = 7000) {
        clearTimeout(timer); region.replaceChildren(tag(message, tone));
        region.hidden = false;
        const delay = Number(duration);
        timer = setTimeout(() => { region.hidden = true; region.replaceChildren(); }, Number.isFinite(delay) && delay >= 0 ? delay : 7000);
      },
      destroy() { clearTimeout(timer); region.remove(); }
    };
  }
  function renderJson(target, value) {
    const text = JSON.stringify(value, null, 2);
    if (text === undefined) throw new TypeError("Value must be JSON serializable");
    const code = element("code"), tokens = /"(?:\\.|[^"\\])*"|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?|\b(?:true|false|null)\b/g;
    let end = 0;
    for (const match of text.matchAll(tokens)) {
      code.append(document.createTextNode(text.slice(end, match.index)));
      const token = match[0], tail = text.slice(match.index + token.length);
      const kind = token.startsWith('"') ? /^\s*:/.test(tail) ? "key" : "string" : /^(true|false|null)$/.test(token) ? "boolean" : "number";
      code.append(element("span", token, "wb-code-" + kind)); end = match.index + token.length;
    }
    code.append(document.createTextNode(text.slice(end)));
    target.classList.add("wb-json"); target.replaceChildren(code);
  }
  function pageSlice(rows, page = 1, size = 10) {
    if (!Array.isArray(rows) || !Number.isInteger(size) || size < 1 || !Number.isInteger(page)) throw new TypeError("Expected rows, integer page and positive page size");
    const pages = Math.max(1, Math.ceil(rows.length / size)), current = Math.max(1, Math.min(page, pages));
    return { rows: rows.slice((current - 1) * size, current * size), page: current, pages, total: rows.length };
  }
  function createTable(root, { columns, rows = [], pageSize = 10, label = "資料", searchLabel = "篩選", emptyLabel = "沒有符合的資料", searchText = row => JSON.stringify(row) } = {}) {
    if (!columns?.length) throw new TypeError("Table requires columns");
    pageSlice(rows, 1, pageSize);
    const abort = new AbortController(), shell = element("section", null, "wb-data-table");
    const search = element("input"), filterLabel = element("label", searchLabel, "wb-filter");
    search.type = "search"; filterLabel.append(search);
    const wrap = element("div", null, "wb-table-wrap"), table = element("table", null, "wb-table");
    table.append(element("caption", label));
    const head = element("thead"), header = element("tr"), body = element("tbody");
    columns.forEach(column => { const th = element("th", column.label); th.scope = "col"; header.append(th); });
    head.append(header); table.append(head, body); wrap.append(table);
    const pager = element("nav", null, "wb-pagination"), status = element("span");
    pager.setAttribute("aria-label", label + " 分頁"); status.setAttribute("aria-live", "polite");
    let page = 1, source = rows.slice();
    const previous = button("上一頁", () => { page--; render(); }), next = button("下一頁", () => { page++; render(); });
    pager.append(previous, status, next); shell.append(filterLabel, wrap, pager); root.append(shell);
    function render() {
      const query = search.value.trim().toLocaleLowerCase();
      const filtered = source.filter(row => String(searchText(row)).toLocaleLowerCase().includes(query));
      const result = pageSlice(filtered, page, pageSize); page = result.page; body.replaceChildren();
      result.rows.forEach(row => {
        const tr = element("tr");
        columns.forEach(column => {
          const td = element("td"), value = column.render ? column.render(row) : row[column.key];
          if (value instanceof Node) td.append(value); else td.textContent = value == null ? "--" : String(value);
          tr.append(td);
        }); body.append(tr);
      });
      if (!result.rows.length) { const tr = element("tr"), td = element("td", emptyLabel); td.colSpan = columns.length; tr.append(td); body.append(tr); }
      status.textContent = page + " / " + result.pages + " · " + result.total + " 筆";
      previous.disabled = page <= 1; next.disabled = page >= result.pages;
    }
    search.addEventListener("input", () => { page = 1; render(); }, { signal: abort.signal });
    render();
    return { setRows(value) { pageSlice(value, 1, pageSize); source = value.slice(); page = 1; render(); }, destroy() { abort.abort(); shell.remove(); } };
  }
  global.WorkbenchUI = Object.freeze({ version: "0.1.0", element, button, tag, setAppearance, bindTabs, bindDialog, createToast, renderJson, pageSlice, createTable });
})(globalThis);
