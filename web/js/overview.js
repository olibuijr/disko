import { api, state, navigate } from './api.js';
import { el, button, card, bytes, count, duration, basename, empty, ago, ltr } from './dom.js';
import { chart } from './chart.js';

const KIND = { metadata: 'Changed', subtree: 'Folder rescanned', mount: 'Mounted', unmount: 'Unmounted' };

export function overviewPage(container) {
  let alive = true;
  async function refresh() {
    const status = state.status;
    if (!status) return;
    const [tree, activity, history, roots] = await Promise.all([
      api('tree', { id: 1, limit: 8 }), api('activity'), api('history'), api('roots'),
    ]);
    if (!alive) return;
    state.roots = roots;
    const online = status.writer_alive && status.heartbeat_age_seconds < 30;
    const scanning = online && status.state === 'scanning';
    container.replaceChildren(
      ledgerCard(roots.disks, status, scanning),
      grid(scanCard(status, history, online, scanning), foldersCard(tree)),
      grid(changesCard(activity, scanning), speedCard(history)),
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

function scanCard(status, history, online, scanning) {
  if (!online) {
    const node = card('Daemon not running', `Showing the index as it was ${ago(status.heartbeat)}.`);
    node.append(el('p', 'muted', 'Start it with: sudo systemctl start disko'));
    return node;
  }
  if (status.state === 'waiting for disk space') {
    const node = card('Scan waiting for disk space', 'Less than 1 GiB is free where the index is stored.');
    node.append(el('p', 'muted', 'Changes are still tracked. The full scan starts once space is freed.'));
    return node;
  }
  if (!scanning) {
    const node = card('Watching for changes', 'The index updates as files change.');
    const summary = el('div', 'scan-state');
    summary.append(el('strong', '', count(status.totals.entries)), el('span', '', 'entries indexed'));
    node.append(summary, details([
      ['Last change seen', status.last_event ? ago(status.last_event) : 'None yet'],
      ['Last full scan', ago(status.last_scan)],
      ['It took', duration(Number(status.last_scan_seconds))],
    ]));
    return node;
  }
  const node = card('Scanning', 'Totals are partial until the scan finishes.');
  const now = Date.now() / 1000;
  const elapsed = Math.max(1, now - Number(status.scan_started || now));
  const visited = Math.max(0, status.visited_this_run - Number(status.scan_start_visited || 0));
  const rate = visited / elapsed;
  const expected = history.scans.find(s => s.state === 'complete')?.visited || 0;
  const summary = el('div', 'scan-state');
  summary.append(el('strong', '', count(visited)), el('span', '', expected ? `of about ${count(expected)} entries` : 'entries so far'));
  const track = el('div', expected ? 'progress-track' : 'pulse-track'), fill = el('span');
  if (expected) {
    const share = Math.min(.99, visited / expected);
    fill.style.width = `${share * 100}%`;
    track.setAttribute('role', 'progressbar'); track.setAttribute('aria-valuenow', String(Math.round(share * 100)));
    track.setAttribute('aria-valuemin', '0'); track.setAttribute('aria-valuemax', '100'); track.setAttribute('aria-label', 'Scan progress');
  }
  track.append(fill);
  const left = expected && rate > 0 ? Math.max(0, (expected - visited) / rate) : NaN;
  const path = el('div', 'scan-path');
  path.append(el('span', '', 'Now in'), el('code', '', ltr(status.scan_path || '/')));
  node.append(summary, track, details([
    ['Speed', `${count(Math.round(rate))} per second`],
    ['Running for', duration(elapsed)],
    ['Time left', Number.isFinite(left) ? (visited >= expected ? 'Almost done' : `About ${duration(left)}`) : 'Estimating'],
  ]), path);
  return node;
}

function details(pairs) {
  const node = el('div', 'scan-details');
  for (const [label, value] of pairs) {
    const field = el('div'); field.append(el('label', '', label), el('strong', '', value)); node.append(field);
  }
  return node;
}

function foldersCard(tree) {
  const node = card('Largest folders', 'At the top of the index', button('Open disk tree', () => navigate('tree', 1), 'text-button'));
  const max = Math.max(...tree.children.map(n => n.allocated_bytes), 1);
  for (const item of tree.children) {
    const row = button('', () => navigate('tree', item.kind === 'directory' ? item.id : 1), 'folder-row');
    const copy = el('div', 'folder-copy');
    copy.append(el('strong', '', basename(item.path)), el('small', '', `${count(item.files)} ${item.files === 1 ? 'file' : 'files'}`));
    const size = el('div', 'folder-size', bytes(item.allocated_bytes));
    const bar = el('div', 'folder-bar'), fill = el('span');
    fill.style.width = `${item.allocated_bytes / max * 100}%`;
    bar.append(fill); size.append(bar);
    row.append(copy, size); node.append(row);
  }
  return node;
}

function changesCard(activity, scanning) {
  const node = card('Recent changes', 'What the watcher picked up most recently');
  if (!activity.length) { node.append(empty(scanning ? 'Changes appear here once the first scan is done.' : 'Nothing has changed since the last scan.')); return node; }
  for (const item of activity.slice(0, 8)) {
    const row = el('div', 'activity-row'), copy = el('div');
    const parent = item.path.slice(0, item.path.lastIndexOf('/')) || '/';
    copy.append(el('span', 'activity-name', basename(item.path)), el('span', 'activity-dir mono', ltr(parent)));
    copy.title = item.path;
    const when = el('span', 'activity-when');
    when.append(el('span', '', KIND[item.kind] || item.kind), el('br'), document.createTextNode(ago(item.ts)));
    row.append(copy, when); node.append(row);
  }
  return node;
}

function speedCard(history) {
  const node = card('Indexing speed', 'Entries per second, sampled every 30 seconds for a week');
  node.append(chart(history.samples));
  return node;
}
