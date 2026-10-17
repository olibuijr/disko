import { api } from './api.js';
import { el, card, count, stamp, duration, empty } from './dom.js';
import { chart } from './chart.js';

const REASON = {
  'startup': 'Daemon started',
  'one-shot scan': 'Manual scan',
  'dashboard request': 'Rescan from dashboard',
  'event overflow, watcher error, or scheduled reconciliation': 'Scheduled, or too many changes at once',
};
const STATE = { complete: ['Complete', 'badge'], scanning: ['Running', 'badge active'], interrupted: ['Stopped early', 'badge warn'] };

export function historyPage(container) {
  let alive = true;
  async function refresh() {
    const data = await api('history'); if (!alive) return;
    const complete = data.scans.filter(s => s.state === 'complete');
    const latest = complete[0];
    const activity = card('Indexing speed', 'Entries per second, sampled every 30 seconds for a week');
    activity.classList.add('history-card');
    const summary = el('div', 'history-summary');
    for (const [label, value] of [
      ['Scans recorded', count(data.scans.length)],
      ['Finished', count(complete.length)],
      ['Last full scan took', latest ? duration(latest.finished - latest.started) : 'Not finished yet'],
      ['Entries in it', latest ? count(latest.visited) : '—'],
    ]) {
      const item = el('div'); item.append(el('strong', '', value), el('label', '', label)); summary.append(item);
    }
    activity.append(summary, chart(data.samples));
    const scans = card('Scans', 'Full scans run at startup, once a day, on request, and when changes arrive faster than they can be tracked');
    scans.classList.add('history-card');
    if (!data.scans.length) scans.append(empty('No scans yet. The first one starts with the daemon.'));
    else {
      const wrap = el('div', 'table-wrap'), table = el('table'), head = el('thead'), headers = el('tr');
      for (const [text, cls] of [['Started', ''], ['Result', ''], ['Why', 'hide-narrow'], ['Entries', 'right'], ['Took', 'right'], ['Errors', 'right hide-narrow']]) headers.append(el('th', cls, text));
      head.append(headers); table.append(head);
      const body = el('tbody');
      for (const scan of data.scans) {
        const tr = el('tr'), result = el('td');
        const [label, cls] = STATE[scan.state] || [scan.state, 'badge quiet'];
        result.append(el('span', cls, label));
        const finished = scan.state === 'complete';
        tr.append(
          el('td', 'num', stamp(scan.started)), result,
          el('td', 'muted hide-narrow', REASON[scan.reason] || scan.reason),
          el('td', 'num right', finished ? count(scan.visited) : '—'),
          el('td', 'num right', scan.finished ? duration(scan.finished - scan.started) : 'Running'),
          el('td', 'num right hide-narrow', finished ? count(scan.errors + scan.watch_errors) : '—'),
        );
        body.append(tr);
      }
      table.append(body); wrap.append(table); scans.append(wrap);
    }
    container.replaceChildren(activity, scans);
  }
  refresh().catch(error => { if (alive) container.replaceChildren(empty(error.message)); });
  return { refresh, dispose: () => { alive = false; } };
}
