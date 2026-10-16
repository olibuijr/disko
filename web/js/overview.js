import { api, state, navigate } from './api.js';
import { el, button, card, bytes, count, duration, basename, colors, empty, stamp } from './dom.js';
import { chart } from './chart.js';

export function overviewPage(container) {
  let alive = true, previous = null;
  async function refresh() {
    const status = state.status;
    if (!status) return;
    const [tree, activity, history, roots] = await Promise.all([
      api('tree', { id: 1, limit: 8 }), api('activity'), api('history'), api('roots'),
    ]);
    if (!alive) return;
    state.roots = roots;
    const now = Date.now() / 1000;
    const online = status.writer_alive && status.heartbeat_age_seconds < 30;
    const scanning = online && status.state === 'scanning';
    const elapsed = Math.max(1, now - Number(status.scan_started || now));
    const visited = Math.max(0, status.visited_this_run - Number(status.scan_start_visited || 0));
    const speed = previous && status.visited_this_run >= previous.visited
      ? (status.visited_this_run - previous.visited) / Math.max(.1, now - previous.ts)
      : scanning ? visited / elapsed : 0;
    previous = { visited: status.visited_this_run, ts: now };
    const stats = el('div', 'stats-grid');
    for (const [label, value, foot, icon] of [
      ['Indexed files', count(status.totals.files), `${count(status.totals.entries)} entries in the index`, '◫'],
      ['Allocated space', bytes(status.totals.allocated_bytes), 'Path-based allocation totals', '▤'],
      ['Active watches', count(status.watches), `${count(status.watch_errors)} registration failures this run`, '◉'],
      ['Index size', bytes(status.database_bytes), `${count(status.metadata_errors)} metadata errors this run`, '▦'],
    ]) {
      const stat = el('section', 'card stat');
      const head = el('div', 'stat-label', label); head.append(el('span', 'stat-icon', icon));
      stat.append(head, el('div', 'stat-value', value), el('div', 'stat-foot', foot)); stats.append(stat);
    }
    const grid = el('div', 'overview-grid');
    const scan = card(!online ? 'Writer offline' : scanning ? 'Reconciliation running' : 'Filesystem watcher active', '', el('span', 'badge active', !online ? 'OFFLINE' : scanning ? 'INDEXING' : 'WATCHING'));
    scan.classList.add('scan-card');
    const summary = el('div', 'scan-summary');
    summary.append(el('strong', '', count(scanning ? visited : status.totals.entries)), el('span', '', scanning ? 'entries processed this scan' : 'entries ready to query'));
    const track = el('div', scanning ? 'pulse-track' : 'progress-track');
    const fill = el('span'); if (!scanning) fill.style.width = '100%'; track.append(fill);
    const fields = el('div', 'scan-details');
    for (const [label, value] of [['SCAN RATE', `${count(Math.round(speed))} / sec`], ['ELAPSED', scanning ? duration(elapsed) : duration(status.last_scan_seconds)], ['FRESHNESS', `${status.heartbeat_age_seconds}s ago`]]) {
      const field = el('div'); field.append(el('label', '', label), el('strong', 'mono', value)); fields.append(field);
    }
    const current = el('div', 'scan-path');
    current.append(el('span', '', scanning ? '↳' : '✓'), el('code', '', scanning ? status.scan_path || '/' : `Last reconciliation ${stamp(status.last_scan)}`));
    scan.append(summary, track, fields, current);
    if (scanning) scan.append(el('p', 'card-subtitle', 'Discovering entries. Totals are partial until the scan completes.'));
    const disk = card('Disk capacity', 'Live filesystem allocation');
    const disks = state.roots.disks;
    if (disks.length) {
      const capacity = disks.reduce((sum, d) => sum + (d.capacity || 0), 0);
      const free = disks.reduce((sum, d) => sum + (d.available || 0), 0);
      const value = el('div', 'storage-number', bytes(capacity - free));
      value.append(el('small', '', ` / ${bytes(capacity)}`));
      const bar = el('div', 'storage-chart');
      const used = el('span'); used.style.width = `${(capacity - free) / capacity * 100}%`; used.style.background = '#86d25d';
      bar.append(used);
      disk.append(value, el('p', 'card-subtitle', `${bytes(free)} available across ${disks.length} disks`), bar);
      const legend = el('div', 'legend'); legend.append(el('span', '', '● Used'), el('span', '', '○ Available')); disk.append(legend);
    } else disk.append(empty('Discovering mounted disks…'));
    const folders = card('Top-level allocation', 'Largest folders at the index root', button('Disk Tree ↗', () => navigate('tree', 1), 'text-button'));
    const maximum = Math.max(...tree.children.map(n => n.allocated_bytes), 1);
    for (const [index, item] of tree.children.entries()) {
      const row = button('', () => navigate('tree', item.kind === 'directory' ? item.id : 1), 'folder-row');
      row.style.width = '100%'; row.style.borderTop = '0'; row.style.borderLeft = '0'; row.style.borderRight = '0'; row.style.background = 'transparent'; row.style.textAlign = 'left';
      const icon = el('span', 'folder-icon', item.kind === 'directory' ? '▱' : '·'); icon.style.background = colors[index % colors.length];
      const copy = el('div', 'folder-copy'); copy.append(el('strong', '', basename(item.path)), el('small', '', `${count(item.files)} files`));
      const size = el('div', 'folder-size', bytes(item.allocated_bytes));
      const bar = el('div', 'folder-bar'), fill = el('span'); fill.style.width = `${item.allocated_bytes / maximum * 100}%`; fill.style.background = colors[index % colors.length]; bar.append(fill); size.append(bar);
      row.append(icon, copy, size); folders.append(row);
    }
    const changes = card('Recent changes', 'Updates from the filesystem watcher');
    if (!activity.length) changes.append(empty(scanning ? 'Changes will appear after the first scan.' : 'No recent changes.'));
    for (const item of activity.slice(0, 7)) {
      const row = el('div', 'activity-row'), copy = el('div');
      copy.append(el('code', '', item.path), el('small', '', `${item.kind} · ${stamp(item.ts)}`)); row.append(el('span', 'dot'), copy); changes.append(row);
    }
    grid.append(scan, disk, folders, changes);
    const rate = card('Indexing activity', 'Entries processed per second · samples every 30 seconds');
    rate.classList.add('history-chart'); rate.style.marginTop = '20px'; rate.append(chart(history.samples));
    container.replaceChildren(stats, grid, rate);
  }
  refresh().catch(error => { if (alive) container.replaceChildren(empty(error.message)); });
  return { refresh, dispose: () => { alive = false; } };
}
