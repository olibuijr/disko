import { el, stamp } from './dom.js';
export function chart(samples) {
  const wrap = el('div');
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.classList.add('chart');
  svg.setAttribute('viewBox', '0 0 800 140');
  svg.setAttribute('preserveAspectRatio', 'none');
  svg.setAttribute('role', 'img');
  svg.setAttribute('aria-label', 'Indexing speed over time, entries per second');
  const points = [...samples].reverse().map((item, i, all) => {
    if (!i || item.visited < all[i - 1].visited) return { ts: item.ts, speed: 0 };
    return { ts: item.ts, speed: (item.visited - all[i - 1].visited) / Math.max(1, item.ts - all[i - 1].ts) };
  });
  const add = (tag, attrs) => {
    const node = document.createElementNS(svg.namespaceURI, tag);
    for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
    svg.append(node);
  };
  for (const y of [15, 55, 95, 135]) add('line', { x1: 0, x2: 800, y1: y, y2: y, class: 'chart-grid' });
  if (points.length > 1) {
    const max = Math.max(...points.map(point => point.speed), 1);
    const xy = points.map((point, i) => `${i / (points.length - 1) * 800},${130 - point.speed / max * 110}`).join(' ');
    add('polygon', { points: `0,140 ${xy} 800,140`, class: 'chart-fill' });
    add('polyline', { points: xy, class: 'chart-line' });
  }
  const labels = el('div', 'chart-labels');
  labels.append(el('span', '', points.length ? stamp(points[0].ts) : 'Waiting for samples'), el('span', '', 'entries / sec'), el('span', '', points.length ? stamp(points.at(-1).ts) : ''));
  wrap.append(svg, labels); return wrap;
}
