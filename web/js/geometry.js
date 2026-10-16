// Squarified treemap. Area is proportional to weight; no DOM or data fetching.
export function treemap(items, width, height) {
  const total = items.reduce((sum, item) => sum + item.weight, 0);
  if (!total || width <= 0 || height <= 0) return [];
  const pending = items.map(item => ({ ...item, area: item.weight / total * width * height }))
    .sort((a, b) => b.area - a.area);
  const output = [];
  let x = 0, y = 0, w = width, h = height, row = [];
  const worst = (group, side) => {
    if (!group.length || side <= 0) return Infinity;
    const areas = group.map(item => item.area);
    const sum = areas.reduce((a, b) => a + b, 0);
    return Math.max(side * side * Math.max(...areas) / (sum * sum), sum * sum / (side * side * Math.min(...areas)));
  };
  const place = () => {
    const sum = row.reduce((n, item) => n + item.area, 0);
    if (w >= h) {
      const thickness = sum / h;
      let offset = y;
      for (const item of row) {
        const length = item.area / thickness;
        output.push({ ...item, x, y: offset, width: thickness, height: length });
        offset += length;
      }
      x += thickness; w = Math.max(0, w - thickness);
    } else {
      const thickness = sum / w;
      let offset = x;
      for (const item of row) {
        const length = item.area / thickness;
        output.push({ ...item, x: offset, y, width: length, height: thickness });
        offset += length;
      }
      y += thickness; h = Math.max(0, h - thickness);
    }
    row = [];
  };
  while (pending.length) {
    const item = pending[0];
    if (!row.length || worst([...row, item], Math.min(w, h)) <= worst(row, Math.min(w, h))) {
      row.push(pending.shift());
    } else place();
  }
  if (row.length) place();
  return output;
}
