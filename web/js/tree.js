import { api, state, metric } from './api.js';
import { el, button, bytes, count, basename, colors, empty } from './dom.js';
import { rootDropdown, metricToggle } from './components.js';
import { treemap } from './geometry.js';
import { details } from './details.js';
import { deleteButton } from './delete.js';

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
  const map = el('div', 'treemap');
  map.setAttribute('role', 'group');
  map.setAttribute('aria-label', 'Disk usage treemap. Box area represents space used.');
  const foot = el('div', 'tree-foot');
  const legend = el('div', 'tree-legend');
  for (const [label, color] of [['Folders', colors[0]], ['Files', '#493025'], ['Other', '#263027']]) {
    const item = el('span'), chip = el('i'); chip.style.background = color;
    item.append(chip, document.createTextNode(label)); legend.append(item);
  }
  foot.append(legend, el('span', '', 'Click a folder to explore · Area = space used'));
  card.append(header, map, foot);
  const detail = el('div');
  container.replaceChildren(toolbar, card, el('p', 'tree-note', 'Path-based totals. Shared blocks, snapshots and filesystem overhead can differ from disk capacity. Zero-size entries have no area.'), detail);
  function draw() {
    if (!data || !alive) return;
    const nodes = data.children.map((node, index) => ({ node, weight: metric(node), index })).filter(item => item.weight > 0);
    const sum = nodes.reduce((n, item) => n + item.weight, 0);
    const rest = Math.max(0, metric(data.node) - sum);
    if (rest > 0) nodes.push({ node: null, weight: rest, index: -1 });
    map.replaceChildren();
    if (!nodes.length) { map.append(empty('No allocated space in this folder. Try Apparent to see sparse files.')); return; }
    const boxes = treemap(nodes, map.clientWidth, map.clientHeight);
    for (const box of boxes) {
      const node = box.node;
      const tile = button('', () => {
        if (node?.kind === 'directory') { state.treeId = node.id; load(); }
        else if (node) inspect(node);
      }, 'tree-tile');
      tile.style.left = `${box.x}px`; tile.style.top = `${box.y}px`;
      tile.style.width = `${box.width}px`; tile.style.height = `${box.height}px`;
      tile.style.background = !node ? '#263027' : node.kind === 'directory' ? colors[box.index % colors.length] : '#493025';
      if (box.width < 140 || box.height < 110) tile.classList.add('compact');
      if (box.width < 65 || box.height < 45) tile.classList.add('tiny');
      const name = node ? basename(node.path) : 'Other entries';
      const percent = (box.weight / (sum + rest) * 100).toFixed(1);
      tile.title = `${node?.path || 'Remaining entries and directory blocks'}\n${bytes(box.weight)} · ${percent}%`;
      tile.setAttribute('aria-label', `${name}, ${bytes(box.weight)}, ${percent} percent${node?.kind === 'directory' ? ', open folder' : ''}`);
      const top = el('div', 'tile-top');
      top.append(el('span', 'tile-icon', node?.kind === 'directory' ? '▱' : '·'), el('strong', 'tile-name', name), el('span', 'tile-size', bytes(box.weight)));
      tile.append(top, el('span', 'tile-meta', node ? `${percent}% · ${count(node.files)} ${node.files === 1 ? "file" : "files"}` : `${percent}% of this folder`));
      map.append(tile);
    }
  }
  function inspect(node) {
    const info = details(node);
    info.querySelector(".card-head").append(deleteButton(node, () => { state.treeId = data.ancestors.at(-2)?.id || 1; detail.replaceChildren(); load(); }));
    detail.replaceChildren(info);
  }
  async function load() {
    const request = ++sequence;
    try {
      const result = await api('tree', { id: state.treeId, metric: state.metric, limit: 200 });
      if (!alive || request !== sequence) return;
      data = result;
      crumbs.replaceChildren();
      for (const [i, ancestor] of data.ancestors.entries()) {
        if (i) crumbs.append(el('span', '', '/'));
        crumbs.append(button(ancestor.path === '/' ? '◫ Root' : basename(ancestor.path), () => {
          state.treeId = ancestor.id; load();
        }, ''));
      }
      total.textContent = `${bytes(metric(data.node))} · ${count(data.node.files)} files`;
      if (!detail.children.length || detail.dataset.folder !== String(data.node.id)) { inspect(data.node); detail.dataset.folder = String(data.node.id); } draw();
    } catch (error) { if (alive) map.replaceChildren(empty(error.message)); }
  }
  const observer = new ResizeObserver(draw); observer.observe(map);
  load();
  return { refresh: load, dispose: () => { alive = false; observer.disconnect(); } };
}
