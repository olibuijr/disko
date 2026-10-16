export function el(tag, className = '', text = '') {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== '') node.textContent = text;
  return node;
}
export function button(text, action, className = 'button') {
  const node = el('button', className, text);
  node.type = 'button';
  node.addEventListener('click', action);
  return node;
}
export function card(title, subtitle, action) {
  const node = el('section', 'card');
  const head = el('div', 'card-head');
  const copy = el('div');
  copy.append(el('h2', '', title));
  if (subtitle) copy.append(el('p', 'card-subtitle', subtitle));
  head.append(copy);
  if (action) head.append(action);
  node.append(head);
  return node;
}
export function empty(text) { return el('div', 'empty', text); }
export const bytes = value => {
  let n = Number(value || 0), unit = 0;
  while (n >= 1024 && unit < 5) { n /= 1024; unit++; }
  return `${n.toLocaleString(undefined, { maximumFractionDigits: unit ? 1 : 0 })} ${['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'][unit]}`;
};
export const count = value => Number(value || 0).toLocaleString();
export const basename = path => path === '/' ? '/' : path.split('/').filter(Boolean).at(-1);
export const stamp = ts => ts ? new Date(ts * 1000).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }) : '—';
export function duration(seconds) {
  if (!Number.isFinite(seconds)) return '—';
  if (seconds < 60) return `${Math.floor(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor(seconds % 3600 / 60)}m`;
}
export function toast(text) {
  const node = document.querySelector('#toast');
  node.textContent = text;
  node.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { node.hidden = true; }, 4500);
}
export const colors = ['#244a2c', '#1d4238', '#3e4423', '#203d49', '#424022', '#43302b', '#283e25', '#263d3b'];
