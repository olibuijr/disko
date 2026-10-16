import { api, state } from './api.js';
export function showLogin(onSignedIn) {
  document.body.classList.remove('authenticated');
  const form = document.querySelector('#login-form');
  const input = document.querySelector('#password');
  const error = document.querySelector('#login-error');
  input.value = ''; error.textContent = '';
  form.onsubmit = async event => {
    event.preventDefault();
    const submit = form.querySelector('button'); submit.disabled = true;
    try {
      state.session = await api('login', {}, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ password: input.value }),
      });
      input.value = ''; document.body.classList.add('authenticated'); onSignedIn();
    } catch (failure) { error.textContent = failure.message; }
    finally { submit.disabled = false; }
  };
  input.focus();
}
