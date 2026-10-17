export async function api(route, params = {}, options = {}) {
  const query = new URLSearchParams(params);
  const response = await fetch(`/api/${route}${query.size ? `?${query}` : ''}`, {
    signal: AbortSignal.timeout(12000),
    ...options,
  });
  const data = await response.json();
  if (!response.ok) { const error = new Error(data.error || `Request failed: ${response.status}`); error.status = response.status; throw error; }
  return data;
}
export const state = {
  page: 'overview',
  status: null,
  roots: { indexes: [], disks: [] },
  session: null,
  metric: 'allocated',
  root: { id: 1, path: '/', name: 'Machine index' },
  treeId: 1,
  refresh: null,
};
/**
 * The daemon is alive when its process exists. A late heartbeat on a live
 * process means it is busy (a slow disk sync), not stopped.
 */
export function liveness(status) {
  const alive = Boolean(status.writer_alive);
  const late = alive && status.heartbeat_age_seconds > 45;
  return { alive, late, scanning: alive && status.state === 'scanning' };
}
export function metric(node) {
  return Number(node[state.metric === 'apparent' ? 'apparent_bytes' : 'allocated_bytes'] || 0);
}
export function navigate(page, id) {
  if (id !== undefined) state.treeId = id;
  if (location.hash === `#${page}`) window.dispatchEvent(new Event('hashchange'));
  else location.hash = page;
}
