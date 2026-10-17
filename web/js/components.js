import { el, button, bytes, count, basename, ago, ltr } from './dom.js';
import { api, state, metric, navigate } from './api.js';

export function metricToggle(onChange) {
  const node = el('div', 'segmented');
  for (const [value, label] of [['allocated', 'Space on disk'], ['apparent', 'File size']]) {
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
    copy.append(el('strong', '', item.capacity ? item.name : item.path === '/' ? 'All disks' : item.path));
    copy.append(el('small', '', item.capacity ? `${bytes(item.available)} free of ${bytes(item.capacity)}` : item.path === '/' ? 'Every indexed disk' : item.path));
    return copy;
  };
  const update = () => {
    trigger.replaceChildren(el('span', 'drive-icon', '▤'), label(state.root), el('span', 'dropdown-chevron', '⌄'));
    menu.replaceChildren();
    for (const [key, title] of [['indexes', 'Everything'], ['disks', 'One disk']]) {
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

export function fileTable(nodes, onFile, folders = false) {
  const wrap = el('div', 'table-wrap');
  if (!nodes.length) { wrap.append(el('div', 'empty', 'Nothing indexed here yet.')); return wrap; }
  const table = el('table');
  const head = el('thead'), row = el('tr');
  const columns = [['Name', ''], [state.metric === 'apparent' ? 'File size' : 'Space on disk', ''], folders ? ['Files', 'right hide-narrow'] : ['Modified', 'hide-narrow']];
  for (const [text, cls] of columns) row.append(el('th', cls, text));
  head.append(row); table.append(head);
  const body = el('tbody');
  const max = Math.max(...nodes.map(metric), 1);
  for (const item of nodes) {
    const tr = el('tr'), path = el('td', 'path-cell');
    const folder = item.kind === 'directory';
    const link = button('', () => folder ? navigate('tree', item.id) : onFile?.(item), '');
    link.title = folder ? `Open ${item.path} in the disk tree` : `Details for ${item.path}`;
    link.dataset.key = `row:${item.id}`;
    const name = el('strong');
    if (folder) name.append(el('span', 'kind-icon', '▱'));
    name.append(document.createTextNode(basename(item.path)));
    link.append(name, el('small', 'mono', ltr(item.path)));
    path.append(link);
    const size = el('td', 'row-size', bytes(metric(item)));
    const bar = el('div', 'row-bar'), fill = el('span');
    fill.style.width = `${Math.max(metric(item) / max * 100, .5)}%`;
    bar.append(fill); size.append(bar);
    tr.append(path, size, folders ? el('td', 'num right muted hide-narrow', count(item.files)) : el('td', 'when hide-narrow', ago(item.mtime)));
    body.append(tr);
  }
  table.append(body); wrap.append(table); return wrap;
}
