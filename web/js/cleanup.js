import { api, state, navigate } from './api.js';
import { el, button, card, bytes, empty, ago, ltr } from './dom.js';
import { deleteButton } from './delete.js';

/** Build output, package folders and caches, largest first. */
export function cleanupPage(container) {
  let alive = true, shown = '';
  const summary = card('Rebuildable folders', 'Tools recreate these on demand. Deleting one frees its space; the next build or install takes longer.');
  const body = el('div');
  summary.append(body);
  container.replaceChildren(summary);
  async function refresh(force = false) {
    const data = await api('cleanup', force ? { refresh: 1 } : {});
    if (!alive) return;
    const signature = JSON.stringify([data.folders.map(f => f.id), data.updating]);
    if (signature === shown) return; // Keep focus and hover when nothing changed.
    shown = signature;
    if (!data.folders.length) {
      body.replaceChildren(empty(data.updating ? 'Looking for build output and caches. On a busy disk this can take a few minutes.' : 'No rebuildable folders over 64 MiB were found.'));
      return;
    }
    const total = data.folders.reduce((sum, f) => sum + f.allocated_bytes, 0);
    const head = el('p', 'cleanup-total');
    head.append(el('strong', '', bytes(total)), document.createTextNode(` in ${data.folders.length} folders`));
    if (data.updating) head.append(el('span', 'muted', ', refreshing'));
    else if (data.age_seconds > 60) head.append(el('span', 'muted', `, checked ${ago(Date.now() / 1000 - data.age_seconds)}`));
    const wrap = el('div', 'table-wrap'), table = el('table'), thead = el('thead'), hr = el('tr');
    for (const [text, cls] of [['Folder', ''], ['What it is', 'hide-narrow'], ['Space on disk', ''], ['Modified', 'hide-narrow'], ['', 'right']]) hr.append(el('th', cls, text));
    thead.append(hr); table.append(thead);
    const tbody = el('tbody');
    for (const item of data.folders) {
      const tr = el('tr'), path = el('td', 'path-cell');
      const link = button('', () => navigate('tree', item.id), '');
      link.dataset.key = `clean:${item.id}`;
      link.title = `Open ${item.path} in the disk tree`;
      // Many rows share a name like target or node_modules; lead with the project.
      link.append(el('strong', '', item.path.split('/').filter(Boolean).slice(-2).join('/')), el('small', 'mono', ltr(item.path)));
      path.append(link);
      const action = el('td', 'right');
      const remove = deleteButton(item, () => { shown = ''; refresh(true); });
      remove.setAttribute?.('aria-label', `Delete ${item.path}`);
      action.append(remove);
      tr.append(path, el('td', 'muted hide-narrow', item.reason), el('td', 'row-size', bytes(item.allocated_bytes)), el('td', 'when hide-narrow', ago(item.mtime)), action);
      tbody.append(tr);
    }
    table.append(tbody); wrap.append(table);
    body.replaceChildren(head, wrap);
  }
  refresh().catch(error => { if (alive) body.replaceChildren(empty(error.message)); });
  return { refresh: () => refresh(), dispose: () => { alive = false; } };
}
