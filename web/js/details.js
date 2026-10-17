import { el, card, bytes, count, stamp } from './dom.js';
export function details(item) {
  const node = card(item.kind === 'directory' ? 'This folder' : 'This file', item.path);
  node.classList.add('tree-info');
  const grid = el('div', 'details-grid');
  const folder = item.kind === 'directory';
  for (const [label, value] of [
    ['Space on disk', bytes(item.allocated_bytes)],
    ['File size', bytes(item.apparent_bytes)],
    folder ? ['Contains', `${count(item.files)} files, ${count(item.entries - item.files)} folders`] : ['Hard links', count(item.links)],
    ['Modified', stamp(item.mtime)],
    ['Owner', `uid ${item.uid}, gid ${item.gid}`],
    ['Inode', String(item.inode)],
  ]) {
    const field = el('div');
    field.append(el('label', '', label), el('strong', '', value));
    grid.append(field);
  }
  node.append(grid); return node;
}
