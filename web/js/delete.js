import { api, state } from './api.js';
import { el, button, bytes, count, toast } from './dom.js';

export function deleteButton(item, onDeleted) {
  if (!state.session?.can_rescan) return el('span');
  return button('Delete…', async () => {
    try {
      const preview = await api('delete/preview', {}, {
        method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Disko-Token': state.session.token },
        body: JSON.stringify({ id: item.id }),
      });
      showConfirmation(preview, onDeleted);
    } catch (error) { toast(error.message); }
  }, 'button danger-outline');
}
function showConfirmation(preview, onDeleted) {
  const dialog = el('dialog', 'delete-dialog');
  dialog.setAttribute('aria-labelledby', 'delete-title');
  dialog.setAttribute('aria-describedby', 'delete-description');
  const title = el('h2', '', `Delete this ${preview.entry.kind === 'directory' ? 'folder' : 'file'}?`);
  title.id = 'delete-title';
  const description = el('p', '', 'This permanently removes the item from the filesystem. Folders include their contents.');
  description.id = 'delete-description';
  const info = el('div', 'delete-entry');
  info.append(el('code', '', preview.entry.path), el('small', '', `${bytes(preview.entry.allocated_bytes)} indexed · ${count(preview.entry.files)} files`));
  const error = el('p', 'delete-error'); error.setAttribute('role', 'alert');
  const actions = el('div', 'dialog-actions');
  const cancel = button('Cancel', () => dialog.close(), 'button secondary');
  const confirm = button('Delete', async () => {
    confirm.disabled = true; cancel.disabled = true; confirm.textContent = 'Deleting…';
    try {
      await api('delete', {}, {
        method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Disko-Token': state.session.token },
        body: JSON.stringify({ token: preview.token }),
      });
      dialog.close(); toast('Deleted. The index will update from filesystem events.'); onDeleted?.();
    } catch (failure) {
      error.textContent = failure.message;
      cancel.disabled = false; confirm.textContent = 'Delete';
    }
  }, 'button danger');
  actions.append(cancel, confirm); dialog.append(el('div', 'delete-symbol', '×'), title, description, info, error, actions);
  document.body.append(dialog);
  dialog.addEventListener('close', () => dialog.remove(), { once: true });
  dialog.showModal(); cancel.focus();
}
