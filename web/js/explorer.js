import { api, state } from './api.js';
import { el, button, card, empty } from './dom.js';
import { rootDropdown, metricToggle, fileTable } from './components.js';
import { details } from './details.js';
import { deleteButton } from './delete.js';

export function explorerPage(container) {
  let alive = true, kind = 'files', sequence = 0, timer, shown = '';
  const toolbar = el('div', 'toolbar'), group = el('div', 'toolbar-group');
  group.append(rootDropdown(load), metricToggle(load));
  const types = el('div', 'segmented');
  for (const value of ['files', 'dirs']) {
    const item = button(value === 'files' ? 'Files' : 'Folders', () => {
      kind = value;
      for (const child of types.children) child.classList.toggle('active', child === item);
      load();
    }, value === kind ? 'active' : ''); types.append(item);
  }
  toolbar.append(group, types);
  const search = el('label', 'search-box'); search.append(el('span', '', '⌕')); search.querySelector('span').setAttribute('aria-hidden', 'true');
  const input = el('input'); input.type = 'search'; input.placeholder = 'Search names that start with…'; input.setAttribute('aria-label', 'Search by the start of a file or folder name'); search.append(input);
  const results = card('Largest files', 'Select a file for details, or a folder to open it in the disk tree.', search);
  const table = el('div'), detail = el('div'); results.append(table);
  container.replaceChildren(toolbar, results, detail);
  input.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(load, 250); });
  function inspect(item) {
    const info = details(item);
    info.querySelector('.card-head').append(deleteButton(item, () => { detail.replaceChildren(); load(); }));
    detail.replaceChildren(info);
  }
  async function load() {
    const request = ++sequence;
    try {
      const text = input.value.trim();
      const rows = await api(text ? 'find' : 'largest', text ? { q: text, limit: 100 } : { id: state.root.id, kind, metric: state.metric, limit: 100 });
      if (!alive || sequence !== request) return;
      const signature = JSON.stringify([text, kind, state.metric, rows.map(r => [r.id, r.allocated_bytes, r.apparent_bytes])]);
      if (signature === shown) return; // Unchanged: keep focus and hover in place.
      shown = signature;
      results.querySelector('h2').textContent = text ? `Names starting with “${text}”` : `Largest ${kind === 'files' ? 'files' : 'folders'}`;
      table.replaceChildren(fileTable(rows, inspect, !text && kind === 'dirs'));
    } catch (error) { if (alive) table.replaceChildren(empty(error.message)); }
  }
  load();
  return { refresh: load, dispose: () => { alive = false; clearTimeout(timer); } };
}
