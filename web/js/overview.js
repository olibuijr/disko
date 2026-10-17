import { api, state, navigate, liveness } from './api.js';
import { el, button, card, bytes, count, empty, ago } from './dom.js';
import { scanCard, foldersCard, changesCard, speedCard, growthCard } from './cards.js';

export function overviewPage(container) {
  let alive = true, history = null, historyAt = 0;
  async function refresh() {
    const status = state.status;
    if (!status) return;
    // History is large and changes every 30 seconds at most.
    const historyDue = !history || Date.now() - historyAt > 30000;
    const [tree, activity, fresh, roots, grew] = await Promise.all([
      api('tree', { id: 1, limit: 8 }), api('activity'), historyDue ? api('history') : null, api('roots'),
      api('growth', { days: 7 }),
    ]);
    if (!alive) return;
    if (fresh) { history = fresh; historyAt = Date.now(); }
    state.roots = roots;
    const live = liveness(status);
    container.replaceChildren(
      ledgerCard(roots.disks, status, live.scanning),
      grid(scanCard(status, history, live), growthCard(grew)),
      grid(foldersCard(tree), changesCard(activity, live.scanning)),
      speedCard(history),
    );
  }
  refresh().catch(error => { if (alive) container.replaceChildren(empty(error.message)); });
  return { refresh, dispose: () => { alive = false; } };
}

function grid(...cards) {
  const node = el('div', 'overview-grid');
  node.append(...cards);
  return node;
}

/** Per disk: space in the index, space the index can't see, and free space. */
function ledgerCard(disks, status, scanning) {
  const node = card('Space on each disk', scanning ? 'Indexed totals are still growing while the scan runs.' : 'Select a disk to open it in the disk tree.');
  if (!disks.length) { node.append(empty('No disks found in the index yet.')); return node; }
  const list = el('div', 'ledger');
  let hidden = false;
  for (const disk of [...disks].sort((a, b) => b.capacity - a.capacity)) {
    const capacity = Math.max(1, disk.capacity);
    const inIndex = Math.min(disk.indexed, disk.used);
    const outside = Math.max(0, disk.used - disk.indexed);
    const reserved = Math.max(0, disk.capacity - disk.used - disk.available);
    if (outside > capacity * .005) hidden = true;
    const row = button('', () => { state.root = disk; state.treeId = disk.id; navigate('tree', disk.id); }, 'ledger-disk');
    row.dataset.key = `disk:${disk.path}`;
    row.setAttribute('aria-label', `${disk.name}: ${bytes(disk.used)} used of ${bytes(disk.capacity)}, ${bytes(outside)} not in the index, ${bytes(disk.available)} free. Open in disk tree.`);
    const top = el('div', 'ledger-top');
    const name = el('span', 'ledger-name', disk.name);
    name.append(el('small', '', `${disk.mounts.length > 1 ? `${disk.mounts.length} mounts` : disk.path}, ${disk.filesystem}`));
    const total = el('span', 'ledger-total');
    total.append(el('strong', '', bytes(disk.used)), document.createTextNode(` used of ${bytes(disk.capacity)}`));
    top.append(name, total);
    const bar = el('div', 'ledger-bar');
    for (const [cls, value] of [['in', inIndex], ['out', outside], ['other', reserved]]) {
      if (value <= 0) continue;
      const part = el('span', cls); part.style.width = `${value / capacity * 100}%`; bar.append(part);
    }
    const figures = el('div', 'ledger-figures');
    const figure = (key, label, value) => {
      const item = el('span'); item.append(el('i', `key ${key}`), document.createTextNode(`${label} `), el('b', '', bytes(value)));
      figures.append(item);
    };
    figure('in', 'In the index', inIndex);
    if (outside > capacity * .005) figure('out', 'Not in the index', outside);
    if (reserved > capacity * .005) figure('other', 'Reserved', reserved);
    figure('free', 'Free', disk.available);
    if (disk.indexed > disk.used * 1.02 && !scanning) {
      figures.append(el('span', '', `The index counts ${bytes(disk.indexed - disk.used)} more than the disk uses (compression or shared blocks).`));
    }
    row.append(top, bar, figures);
    list.append(row);
  }
  node.append(list);
  if (hidden) {
    const note = el('div', 'ledger-note');
    note.append(el('span', '', 'Not in the index is space a disk uses that no indexed file accounts for. It is usually snapshots, which keep deleted files on disk, plus excluded folders and filesystem metadata. Deleting files frees little of it until the snapshots holding them are removed.'));
    node.append(note);
  }
  const facts = el('div', 'facts');
  for (const [label, value] of [
    ['Files indexed', count(status.totals.files)],
    ['Folders and links', count(status.totals.entries - status.totals.files)],
    ['Index size', bytes(status.database_bytes)],
    ['Last full scan', status.last_scan ? ago(status.last_scan) : 'Not finished yet'],
  ]) {
    const item = el('div'); item.append(el('strong', '', value), document.createTextNode(label)); facts.append(item);
  }
  node.append(facts);
  node.classList.add('wide');
  return node;
}
