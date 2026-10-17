import { api, state } from './api.js';
import { toast, count, ago } from './dom.js';
import { overviewPage } from './overview.js';
import { treePage } from './tree.js';
import { explorerPage } from './explorer.js';
import { historyPage } from './history.js';
import { showLogin } from './login.js';

const pages = {
  overview: { title: 'Overview', subtitle: 'How space is used on each disk, and what the index is doing.', render: overviewPage },
  tree: { title: 'Disk tree', subtitle: 'Every box is sized by the space it uses. Open folders to find what is large.', render: treePage },
  explorer: { title: 'Explorer', subtitle: 'The largest files and folders, or search by name.', render: explorerPage },
  history: { title: 'History', subtitle: 'Past scans and how fast the index was built.', render: historyPage },
};
let current = null, busy = false, polling = null;
function route() {
  current?.dispose();
  state.page = location.hash.slice(1) in pages ? location.hash.slice(1) : 'overview';
  const page = pages[state.page];
  document.querySelector('#page-title').textContent = page.title;
  document.querySelector('#page-subtitle').textContent = page.subtitle;
  for (const link of document.querySelectorAll('[data-page]')) {
    const active = link.dataset.page === state.page;
    link.classList.toggle('active', active);
    if (active) link.setAttribute('aria-current', 'page'); else link.removeAttribute('aria-current');
  }
  document.title = `${page.title} · Disko`;
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
    const scanning = healthy && status.state === 'scanning';
    connected.classList.toggle('offline', !healthy);
    connected.classList.toggle('scanning', scanning);
    connected.lastChild.textContent = !healthy ? 'Daemon not running'
      : scanning ? 'Scanning'
      : status.state === 'waiting for disk space' ? 'Scan waiting for disk space'
      : 'Up to date';
    document.querySelector('#last-updated').textContent = healthy ? '' : `Last heard from ${ago(status.heartbeat)}`;
    const notice = document.querySelector('#error');
    const problems = [];
    if (status.watch_errors) problems.push(`${count(status.watch_errors)} folders can't be watched for changes; they refresh on the next scan.`);
    if (status.metadata_errors) problems.push(`${count(status.metadata_errors)} entries couldn't be read (often permissions); the daemon log lists them.`);
    notice.textContent = problems.join(' '); notice.hidden = !problems.length;
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
    document.querySelector('#version').textContent = `Version ${state.session.version}`;
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
    toast('Rescan started. Totals update while it runs.');
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
