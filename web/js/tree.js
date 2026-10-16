import { api, state, metric } from './api.js';
import { el, button, bytes, count, basename, colors, empty } from './dom.js';
import { rootDropdown, metricToggle } from './components.js';
import { treemap } from './geometry.js';
import { details } from './details.js';
import { deleteButton } from './delete.js';

const KIND_MAP = { mp4: '▶', mkv: '▶', mov: '▶', avi: '▶', webm: '▶', mp3: '♪', flac: '♪', wav: '♪', ogg: '♪', jpg: '◐', jpeg: '◐', png: '◐', gif: '◐', webp: '◐', svg: '◐', heic: '◐', zip: '▣', tar: '▣', gz: '▣', xz: '▣', '7z': '▣', rar: '▣', iso: '▣', dmg: '▣', db: '▤', sqlite: '▤', bak: '▤', log: '▤', js: '⌨', ts: '⌨', py: '⌨', rs: '⌨', go: '⌨', c: '⌨', cpp: '⌨', html: '⌨', css: '⌨' };
const KIND_ICON = path => {
  if (!path) return '◨';
  const base = path.split('/').filter(Boolean).pop() || '';
  return KIND_MAP[base.includes('.') ? base.split('.').pop().toLowerCase() : ''] || (base.includes('.') ? '·' : '▱');
};

function shade(hex, amount) {
    const n = parseInt(hex.slice(1), 16);
    const f = v => Math.max(0, Math.min(255, Math.round(v + amount)));
    return `rgb(${f(n >> 16)},${f((n >> 8) & 255)},${f(n & 255)})`;
  }

export function treePage(container) {
  let data = null, alive = true, sequence = 0;
  const toolbar = el('div', 'toolbar');
  const controls = el('div', 'toolbar-group');
  controls.append(rootDropdown(load));
  toolbar.append(controls, metricToggle(load));
  const card = el('section', 'card tree-card');
  const header = el('div', 'tree-header');
  const crumbs = el('div', 'breadcrumbs');
  crumbs.setAttribute('aria-label', 'Folder breadcrumbs');
  const total = el('span', 'tree-total');
  header.append(crumbs, total);
  const body = el('div', 'tree-body');
  const map = el('div', 'treemap');
  map.setAttribute('role', 'group');
  map.setAttribute('aria-label', 'Disk usage treemap. Box area represents space used. Drag to pan, scroll to zoom.');
  const canvas = el('div', 'tree-canvas');
  map.append(canvas);
  const zoomBar = el('div', 'zoom-bar');
  const zoomBtn = (text, label, action) => { const b = button(text, action, 'zoom-btn'); b.title = label; b.setAttribute('aria-label', label); return b; };
  const zoomOut = zoomBtn('−', 'Zoom out', () => setZoom(view.z / 1.25));
  const zoomLevel = el('span', 'zoom-level', '100%');
  const zoomIn = zoomBtn('+', 'Zoom in', () => setZoom(view.z * 1.25));
  const zoomReset = zoomBtn('⤾', 'Reset pan and zoom', resetView);
  zoomBar.append(zoomOut, zoomLevel, zoomIn, zoomReset);
  map.append(zoomBar);
  const view = { z: 1, x: 0, y: 0 };
  function applyView() {
    canvas.style.transform = `translate(${view.x}px, ${view.y}px) scale(${view.z})`;
    zoomLevel.textContent = `${Math.round(view.z * 100)}%`;
    zoomReset.disabled = view.z === 1 && view.x === 0 && view.y === 0;
  }
  function clampView() {
    const w = map.clientWidth, h = map.clientHeight;
    const zw = w * view.z, zh = h * view.z;
    view.x = zw <= w ? (w - zw) / 2 : Math.min(0, Math.max(w - zw, view.x));
    view.y = zh <= h ? (h - zh) / 2 : Math.min(0, Math.max(h - zh, view.y));
  }
  function setZoom(next, cx, cy) {
    const z = Math.min(6, Math.max(1, next));
    if (z !== view.z) {
      const rect = map.getBoundingClientRect();
      cx ??= rect.width / 2; cy ??= rect.height / 2;
      view.x = cx - (cx - view.x) * (z / view.z);
      view.y = cy - (cy - view.y) * (z / view.z);
      view.z = z;
    }
    clampView(); applyView();
  }
  function resetView() { view.z = 1; view.x = 0; view.y = 0; applyView(); }
  let drag = null, suppressClick = false;
  map.addEventListener('pointerdown', event => {
    if (event.button !== 0 || event.target.closest('.zoom-bar')) return;
    suppressClick = false;
    drag = { sx: event.clientX - view.x, sy: event.clientY - view.y, pid: event.pointerId };
  });
  window.addEventListener('pointermove', event => {
    if (!drag || (drag.pid !== undefined && event.pointerId !== drag.pid)) return;
    const nx = event.clientX - drag.sx, ny = event.clientY - drag.sy;
    if (!suppressClick && Math.abs(nx - view.x) + Math.abs(ny - view.y) <= 3) return;
    suppressClick = true; map.classList.add('panning');
    view.x = nx; view.y = ny;
    clampView(); applyView();
  });
  for (const end of ['pointerup', 'pointercancel']) window.addEventListener(end, () => { drag = null; map.classList.remove('panning'); });
  map.addEventListener('click', event => { if (suppressClick) { event.stopPropagation(); event.preventDefault(); suppressClick = false; } }, true);
  map.addEventListener('wheel', event => {
    event.preventDefault();
    const rect = map.getBoundingClientRect();
    setZoom(view.z * (event.deltaY < 0 ? 1.12 : 1 / 1.12), event.clientX - rect.left, event.clientY - rect.top);
  }, { passive: false });
  map.addEventListener('dblclick', event => {
    if (event.target.closest('.zoom-bar')) return;
    const rect = map.getBoundingClientRect();
    if (view.z === 1) setZoom(2, event.clientX - rect.left, event.clientY - rect.top); else resetView();
  });
  const rank = el('div', 'tree-rank');
  const rankHead = el('div', 'rank-head');
  rankHead.append(el('strong', '', 'Largest in this folder'), el('span', 'rank-count', ''));
  const rankList = el('ol', 'rank-list');
  rank.append(rankHead, rankList);
  body.append(map, rank);
  const foot = el('div', 'tree-foot');
  const legend = el('div', 'tree-legend');
  for (const [label, color] of [['Folders', colors[0]], ['Files', '#493025'], ['Other', '#263027']]) {
    const item = el('span'), chip = el('i'); chip.style.background = color;
    item.append(chip, document.createTextNode(label)); legend.append(item);
  }
  foot.append(legend, el('span', '', 'Drag to pan · Scroll to zoom · Double-click to zoom · Click a folder to explore'));
  card.append(header, body, foot);
  const detail = el('div');
  container.replaceChildren(toolbar, card, el('p', 'tree-note', 'Path-based totals. Shared blocks, snapshots and filesystem overhead can differ from disk capacity. Zero-size entries have no area.'), detail);
  function draw() {
    if (!data || !alive) return;
    const kids = [...data.children].sort((a, b) => metric(b) - metric(a));
    const nodes = kids.map((node, index) => ({ node, weight: metric(node), index })).filter(item => item.weight > 0);
    const sum = nodes.reduce((n, item) => n + item.weight, 0);
    const rest = Math.max(0, metric(data.node) - sum);
    if (rest > 0) nodes.push({ node: null, weight: rest, index: -1 });
    canvas.replaceChildren();
    canvas.style.width = `${map.clientWidth}px`; canvas.style.height = `${map.clientHeight}px`;
    if (!nodes.length) { canvas.append(empty('No allocated space in this folder. Try Apparent to see sparse files.')); return; }
    const totalWeight = sum + rest;
    const boxes = treemap(nodes, map.clientWidth, map.clientHeight);
    for (const box of boxes) {
      const node = box.node;
      const base = !node ? '#263027' : node.kind === 'directory' ? colors[box.index % colors.length] : '#493025';
      const tile = button('', () => {
        if (node?.kind === 'directory') { state.treeId = node.id; load(); }
        else if (node) inspect(node);
      }, 'tree-tile');
      tile.style.left = `${box.x}px`; tile.style.top = `${box.y}px`;
      tile.style.width = `${box.width}px`; tile.style.height = `${box.height}px`;
      tile.style.background = `linear-gradient(160deg, ${shade(base, 22)} 0%, ${base} 55%, ${shade(base, -18)} 100%)`;
      tile.style.setProperty('--tile-accent', !node ? '#7fa88f' : node.kind === 'directory' ? '#a2f779' : '#e8b26a');
      if (box.width < 150 || box.height < 120) tile.classList.add('compact');
      if (box.width < 80 || box.height < 56) tile.classList.add('tiny');
      const name = node ? basename(node.path) : 'Other entries';
      const percent = (box.weight / totalWeight * 100);
      const pctLabel = percent < 0.1 ? '<0.1%' : `${percent.toFixed(1)}%`;
      tile.title = `${node?.path || 'Remaining entries and directory blocks'}\n${bytes(box.weight)} · ${pctLabel}${node?.kind === 'directory' ? '\nClick to open' : ''}`;
      tile.setAttribute('aria-label', `${name}, ${bytes(box.weight)}, ${pctLabel}${node?.kind === 'directory' ? ', open folder' : ''}`);
      const row = el('div', 'tile-row');
      row.append(el('span', 'tile-icon', KIND_ICON(node?.path)), el('strong', 'tile-name', name));
      const meta = el('span', 'tile-meta');
      const bar = el('i', 'tile-bar'); bar.style.width = `${Math.max(2, Math.min(100, percent))}%`;
      meta.append(bar, document.createTextNode(node ? `${pctLabel} · ${count(node.files)} ${node.files === 1 ? 'file' : 'files'}${node.kind === 'directory' ? ' ›' : ''}` : `${pctLabel} of this folder`));
      canvas.appendChild(tile).append(row, el('span', 'tile-size', bytes(box.weight)), meta);
    }
    clampView(); applyView();
    rankList.replaceChildren();
    const ranked = kids.filter(n => metric(n) > 0).slice(0, 30);
    rankHead.querySelector('.rank-count').textContent = `${ranked.length} shown · ${bytes(totalWeight)} total`;
    for (const node of ranked) {
      const w = metric(node), pct = w / totalWeight * 100;
      const li = el('li', 'rank-row');
      const btn = button('', () => {
        if (node.kind === 'directory') { state.treeId = node.id; load(); }
        else inspect(node);
      }, 'rank-btn');
      btn.title = `${node.path}\n${bytes(w)} · ${pct < 0.1 ? '<0.1' : pct.toFixed(1)}%`;
      const fill = el('i', 'rank-fill');
      fill.style.cssText = `width:${Math.max(1.5, Math.min(100, pct))}%;background:${node.kind === 'directory' ? 'linear-gradient(90deg,#5da267,#a2f779)' : 'linear-gradient(90deg,#8a5f36,#e8b26a)'}`;
      const rtop = el('div', 'rank-top');
      rtop.append(el('span', 'rank-icon', node.kind === 'directory' ? '▱' : KIND_ICON(node.path)), el('span', 'rank-name', basename(node.path)), el('span', 'rank-size', bytes(w)));
      const rmeta = el('div', 'rank-meta');
      rmeta.append(el('span', 'rank-bar', ''), el('span', 'rank-pct', pct < 0.1 ? '<0.1%' : `${pct.toFixed(1)}%`), el('span', 'rank-files', `${count(node.files)} ${node.files === 1 ? 'file' : 'files'}${node.kind === 'directory' ? ' ›' : ''}`));
      rmeta.querySelector('.rank-bar').append(fill);
      btn.append(rtop, rmeta);
      li.append(btn); rankList.append(li);
    }
  }
  function inspect(node) {
    const info = details(node);
    info.querySelector(".card-head").append(deleteButton(node, () => { state.treeId = data.ancestors.at(-2)?.id || 1; detail.replaceChildren(); load(); }));
    detail.replaceChildren(info);
  }
  let viewFolder = null;
  async function load() {
    const request = ++sequence;
    try {
      const result = await api('tree', { id: state.treeId, metric: state.metric, limit: 200 });
      if (!alive || request !== sequence) return;
      data = result;
      if (viewFolder !== data.node.id) { viewFolder = data.node.id; resetView(); }
      crumbs.replaceChildren();
      for (const [i, ancestor] of data.ancestors.entries()) {
        if (i) crumbs.append(el('span', '', '/'));
        crumbs.append(button(ancestor.path === '/' ? '◫ Root' : basename(ancestor.path), () => {
          state.treeId = ancestor.id; load();
        }, ''));
      }
      total.textContent = `${bytes(metric(data.node))} · ${count(data.node.files)} files`;
      if (!detail.children.length || detail.dataset.folder !== String(data.node.id)) { inspect(data.node); detail.dataset.folder = String(data.node.id); } draw();
    } catch (error) { if (alive) canvas.replaceChildren(empty(error.message)); }
  }
  const observer = new ResizeObserver(draw); observer.observe(map);
  load();
  return { refresh: load, dispose: () => { alive = false; observer.disconnect(); } };
}
