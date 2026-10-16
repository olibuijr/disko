import { el, button, bytes, count, basename } from './dom.js';
import { api, state, metric, navigate } from './api.js';

export function metricToggle(onChange) {
  const node = el('div', 'segmented');
  for (const [value, label] of [['allocated', 'Allocated'], ['apparent', 'Apparent']]) {
    const item = button(label, () => {
      state.metric = value;
      for (const child of node.children) child.classList.toggle('active', child === item);
      onChange();
    }, value === state.metric ? 'active' : '');
    item.setAttribute('aria-pressed', String(value === state.metric));
    item.addEventListener('click', () => {
      for (const child of node.children) child.setAttribute('aria-pressed', String(child === item));
    });
    node.append(item);
  }
  return node;
}

export function rootDropdown(onChange) {
  const node = el('div', 'dropdown');
  const trigger = el('button', 'dropdown-trigger');
  const menu = el('div', 'dropdown-menu');
  trigger.type = 'button';
  trigger.setAttribute('aria-haspopup', 'listbox');
  trigger.setAttribute('aria-expanded', 'false');
  trigger.setAttribute('aria-label', 'Select disk or index');
  menu.setAttribute('role', 'listbox');
  menu.setAttribute('aria-label', 'Disks and indexes');
  menu.hidden = true;
  const close = () => { menu.hidden = true; trigger.setAttribute('aria-expanded', 'false'); };
  const label = item => {
    const copy = el('div', 'dropdown-copy');
    copy.append(el('strong', '', item.name || item.path));
    copy.append(el('small', '', item.capacity ? `${bytes(item.available)} free of ${bytes(item.capacity)} · ${item.filesystem}` : `${item.path} · persistent metadata index`));
    return copy;
  };
  const update = () => {
    trigger.replaceChildren(el('span', 'drive-icon', '▤'), label(state.root), el('span', 'dropdown-chevron', '⌄'));
    menu.replaceChildren();
    for (const [key, title] of [['indexes', 'INDEXES'], ['disks', 'DISKS']]) {
      if (!state.roots[key].length) continue;
      menu.append(el('div', 'dropdown-section', title));
      for (const item of state.roots[key]) {
        const option = button('', () => {
          state.root = item;
          state.treeId = item.id;
          update(); close(); trigger.focus(); onChange();
        }, 'dropdown-option');
        option.setAttribute('role', 'option');
        option.setAttribute('aria-selected', String(state.root === item));
        option.append(el('span', 'drive-icon', key === 'disks' ? '▤' : '◫'), label(item));
        if (state.root === item) option.append(el('span', 'check', '✓'));
        menu.append(option);
      }
    }
  };
  trigger.addEventListener('click', async () => {
    if (menu.hidden) state.roots = await api('roots').catch(() => state.roots);
    state.root = [...state.roots.indexes, ...state.roots.disks].find(item => item.id === state.root.id && item.path === state.root.path && Boolean(item.capacity) === Boolean(state.root.capacity)) || state.root;
    update();
    menu.hidden = !menu.hidden;
    trigger.setAttribute('aria-expanded', String(!menu.hidden));
    if (!menu.hidden) menu.querySelector('button')?.focus();
  });
  node.addEventListener('keydown', event => {
    if (event.key === 'Escape') { close(); trigger.focus(); }
    if (['ArrowDown', 'ArrowUp'].includes(event.key)) {
      event.preventDefault();
      const items = [...menu.querySelectorAll('button')];
      menu.hidden = false; trigger.setAttribute('aria-expanded', 'true');
      const index = items.indexOf(document.activeElement);
      items[(index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus();
    }
  });
  node.addEventListener('focusout', event => { if (!node.contains(event.relatedTarget)) close(); });
  node.append(trigger, menu); update();
  return node;
}

export function fileTable(nodes, onFile) {
  const wrap = el('div', 'table-wrap');
  if (!nodes.length) { wrap.append(el('div', 'empty', 'No indexed entries here yet.')); return wrap; }
  const table = el('table');
  const head = el('thead'), row = el('tr');
  for (const text of ['NAME / LOCATION', 'TYPE', 'SPACE', 'FILES', 'OWNER']) row.append(el('th', '', text));
  head.append(row); table.append(head);
  const body = el('tbody');
  const max = Math.max(...nodes.map(metric), 1);
  for (const item of nodes) {
    const tr = el('tr'), path = el('td', 'path-cell');
    const link = button('', () => item.kind === 'directory' ? navigate('tree', item.id) : onFile?.(item), '');
    link.append(el('strong', '', `${item.kind === 'directory' ? '▱' : '·'}  ${basename(item.path)}`), el('small', 'mono', item.path));
    path.append(link);
    const size = el('td', 'row-size', bytes(metric(item)));
    const bar = el('div', 'row-bar'), fill = el('span');
    fill.style.width = `${Math.max(metric(item) / max * 100, .5)}%`;
    bar.append(fill); size.append(bar);
    tr.append(path, el('td', 'muted', item.kind), size, el('td', 'mono muted', count(item.files)), el('td', 'mono muted', String(item.uid)));
    body.append(tr);
  }
  table.append(body); wrap.append(table); return wrap;
}
