import { el, card, bytes, count, stamp } from './dom.js';
export function details(item) {
  const node = card('A closer look', item.path);
  node.classList.add('tree-info');
  const grid = el('div', 'details-grid');
  for (const [label, value] of [
    ['ALLOCATED', bytes(item.allocated_bytes)], ['APPARENT', bytes(item.apparent_bytes)],
    ['FILES / ENTRIES', `${count(item.files)} / ${count(item.entries)}`],
    ['LAST MODIFIED', stamp(item.mtime)], ['OWNER', `${item.uid}:${item.gid}`],
    ['INODE / HARDLINKS', `${item.inode} / ${item.links}`],
  ]) {
    const field = el('div');
    field.append(el('label', '', label), el('strong', 'mono', value));
    grid.append(field);
  }
  node.append(grid); return node;
}
