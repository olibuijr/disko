import { api } from './api.js';
import { el, card, count, stamp, duration, empty } from './dom.js';
import { chart } from './chart.js';
export function historyPage(container) {
  let alive = true;
  async function refresh() {
    const data = await api('history'); if (!alive) return;
    const activity = card('Indexing over time', 'Entries per second · history retained for seven days');
    activity.classList.add('history-chart');
    const summary = el('div', 'history-summary');
    for (const [label, value] of [['RECORDED SCANS', count(data.scans.length)], ['COMPLETED', count(data.scans.filter(s => s.state === 'complete').length)], ['LATEST DURATION', duration(data.scans.find(s => s.finished)?.finished - data.scans.find(s => s.finished)?.started)]]) {
      const item = el('div'); item.append(el('label', '', label), el('strong', '', value)); summary.append(item);
    }
    activity.append(summary, chart(data.samples));
    const scans = card('Scan history', 'Startup, scheduled, recovery and requested reconciliations');
    if (!data.scans.length) scans.append(empty('The first scan has not been recorded yet.'));
    else {
      const wrap = el('div', 'table-wrap'), table = el('table'), head = el('thead'), headers = el('tr');
      for (const text of ['STARTED', 'STATUS', 'REASON', 'ENTRIES', 'DURATION', 'ERRORS']) headers.append(el('th', '', text));
      head.append(headers); table.append(head);
      const body = el('tbody');
      for (const scan of data.scans) {
        const tr = el('tr'), status = el('td');
        status.append(el('span', scan.state === 'interrupted' ? 'badge warn' : 'badge', scan.state));
        tr.append(el('td', 'mono', stamp(scan.started)), status, el('td', 'muted', scan.reason), el('td', 'mono', count(scan.visited)), el('td', 'mono', scan.finished ? duration(scan.finished - scan.started) : 'Running'), el('td', 'mono', count(scan.errors + scan.watch_errors)));
        body.append(tr);
      }
      table.append(body); wrap.append(table); scans.append(wrap);
    }
    container.replaceChildren(activity, scans);
  }
  refresh().catch(error => { if (alive) container.replaceChildren(empty(error.message)); });
  return { refresh, dispose: () => { alive = false; } };
}
