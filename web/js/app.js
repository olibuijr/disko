import { api, state } from './api.js';
import { toast } from './dom.js';
import { overviewPage } from './overview.js';
import { treePage } from './tree.js';
import { explorerPage } from './explorer.js';
import { historyPage } from './history.js';
import { showLogin } from './login.js';

const pages = {
  overview: { title: '> status', subtitle: 'Index progress, disk capacity and recent changes.', eyebrow: 'OVERVIEW', render: overviewPage },
  tree: { title: '> disk_tree', subtitle: 'Proportional allocation map. Select a filesystem or descend into a directory.', eyebrow: 'DISK TREE', render: treePage },
  explorer: { title: '> space_explorer', subtitle: 'Browse large files and folders, or search the index.', eyebrow: 'SPACE EXPLORER', render: explorerPage },
  history: { title: '> scan_history', subtitle: 'Reconciliations and indexing activity over time.', eyebrow: 'HISTORY', render: historyPage },
};
let current = null, busy = false, polling = null;
function route() {
  current?.dispose();
  state.page = location.hash.slice(1) in pages ? location.hash.slice(1) : 'overview';
  const page = pages[state.page];
  document.querySelector('#page-title').textContent = page.title;
  document.querySelector('#page-subtitle').textContent = page.subtitle;
  document.querySelector('#page-eyebrow').textContent = page.eyebrow;
  for (const link of document.querySelectorAll('[data-page]')) {
    const active = link.dataset.page === state.page;
    link.classList.toggle('active', active);
    if (active) link.setAttribute('aria-current', 'page'); else link.removeAttribute('aria-current');
  }
  document.title = `${page.eyebrow.toLowerCase().replace(/\b\w/g, c => c.toUpperCase())} · Disko`;
  current = page.render(document.querySelector('#page'));
}
async function poll() {
  if (busy || document.hidden || !state.session?.authenticated) return;
  busy = true;
  try {
    state.status = await api('status');
    const status = state.status;
    const connected = document.querySelector('#connection');
    const healthy = status.writer_alive && status.heartbeat_age_seconds < 30;
    connected.classList.toggle('offline', !healthy);
    connected.lastChild.textContent = healthy ? status.state === 'scanning' ? 'Indexing' : 'Live index' : 'Writer offline';
    document.querySelector('#last-updated').textContent = `Heartbeat ${status.heartbeat_age_seconds}s ago`;
    const notice = document.querySelector('#error');
    const problems = [];
    if (status.watch_errors) problems.push(`${status.watch_errors} watch registration failures. Some changes need reconciliation.`);
    if (status.metadata_errors) problems.push(`${status.metadata_errors} metadata errors. See daemon logs.`);
    notice.textContent = problems.join(' '); notice.hidden = !problems.length;
    document.querySelector('#clock').textContent = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    await current?.refresh();
  } catch (error) {
    if (error.status === 401) { signOut(); return; }
    const notice = document.querySelector('#error'); notice.textContent = error.message; notice.hidden = false;
  } finally { busy = false; }
}
async function start() {
  clearInterval(polling);
  try {
    [state.status, state.roots] = await Promise.all([api('status'), api('roots')]);
    state.root = state.roots.indexes[0] || state.root;
    if (state.treeId === 1) state.treeId = state.root.id;
    document.querySelector('#version').textContent = `Disko v${state.session.version}`;
    document.querySelector('#rescan').disabled = !state.session.can_rescan;
    route(); await poll(); polling = setInterval(poll, 3000);
  } catch (error) { toast(error.message); }
}
function signOut() {
  clearInterval(polling); current?.dispose(); current = null;
  state.session = null; showLogin(start);
}
window.addEventListener('hashchange', () => { if (state.session?.authenticated) route(); });
document.querySelector('#rescan').addEventListener('click', async () => {
  try {
    await api('rescan', {}, { method: 'POST', headers: { 'X-Disko-Token': state.session.token } });
    toast('Reconciliation queued.');
  } catch (error) { toast(error.message); }
});
document.querySelector('#logout').addEventListener('click', async () => {
  await api('logout', {}, { method: 'POST', headers: { 'X-Disko-Token': state.session.token } }).catch(() => {});
  signOut();
});
try {
  state.session = await api('session');
  if (state.session.authenticated) { document.body.classList.add('authenticated'); await start(); }
  else showLogin(start);
} catch (error) { showLogin(start); toast(error.message); }
