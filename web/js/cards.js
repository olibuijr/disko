import { navigate } from './api.js';
import { el, button, card, bytes, count, duration, basename, empty, ago, ltr, stamp } from './dom.js';
import { chart } from './chart.js';

const KIND = { metadata: 'Changed', subtree: 'Folder rescanned', mount: 'Mounted', unmount: 'Unmounted' };

export function scanCard(status, history, live) {
  const scanning = live.scanning;
  if (!live.alive) {
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
  const expected = history?.scans.find(s => s.state === 'complete')?.visited || 0;
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

export function foldersCard(tree) {
  const node = card('Largest folders', 'At the top of the index', button('Open disk tree', () => navigate('tree', 1), 'text-button'));
  const max = Math.max(...tree.children.map(n => n.allocated_bytes), 1);
  for (const item of tree.children) {
    const row = button('', () => navigate('tree', item.kind === 'directory' ? item.id : 1), 'folder-row');
    row.dataset.key = `folder:${item.id}`;
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

export function changesCard(activity, scanning) {
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

export function speedCard(history) {
  const node = card('Indexing speed', 'Entries per second, sampled every 30 seconds for a week');
  node.style.marginTop = '18px';
  node.append(chart(history?.samples || []));
  return node;
}

/** Folders that grew most over the last week, from the daily size record. */
export function growthCard(grew) {
  const node = card('Grew this week', grew.since ? `Largest increases since ${stamp(grew.since)}` : 'Compared with a daily record of large folders');
  if (!grew.since) { node.append(empty('Growth shows up after Disko has recorded folder sizes for a day.')); return node; }
  if (!grew.folders.length) { node.append(empty('No folder grew by more than 256 MiB.')); return node; }
  for (const item of grew.folders) {
    const row = button('', () => navigate('tree', item.id), 'folder-row');
    row.dataset.key = `grew:${item.id}`;
    const copy = el('div', 'folder-copy');
    copy.append(el('strong', '', basename(item.path)), el('small', 'mono', ltr(item.path)));
    copy.lastChild.classList.add('activity-dir');
    const size = el('div', 'folder-size');
    size.append(el('strong', 'grew', `+${bytes(item.grew_bytes)}`), el('br'), el('small', '', `now ${bytes(item.allocated_bytes)}`));
    row.append(copy, size); node.append(row);
  }
  return node;
}
