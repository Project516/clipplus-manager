import { lookupPeer, secureRelayURL } from './soulseek.js';

const $ = (selector) => document.querySelector(selector);
const loginForm = $('#soulseek-login');
const session = $('#soulseek-session');
const status = $('#soulseek-status');
let protocol;
let relay;

function disconnect() {
  relay?.close();
  protocol?.terminate();
  relay = protocol = undefined;
  session.hidden = true;
  loginForm.hidden = false;
}

function submit(form, pending, action, failure) {
  form.addEventListener('submit', async (event) => {
    event.preventDefault();
    const buttons = document.querySelectorAll('#soulseek button');
    for (const button of buttons) button.disabled = true;
    status.textContent = pending;
    try {
      status.textContent = await action();
    } catch {
      disconnect();
      status.textContent = failure;
    } finally {
      for (const button of buttons) button.disabled = false;
    }
  });
}

submit(loginForm, 'Signing in…', async () => {
  let url;
  try {
    url = secureRelayURL($('#relay-url').value);
  } catch {
    return 'Enter a wss:// relay URL. Put the relay token in its own field.';
  }
  const token = $('#relay-token').value;
  const username = $('#soulseek-user').value;
  const password = $('#soulseek-password').value;
  $('#relay-token').value = '';
  $('#soulseek-password').value = '';
  const { createProtocol, connectRelay, login } = await import('@project516/nicotine-plus-wasm');
  protocol = await createProtocol();
  relay = await connectRelay(url, token);
  const result = await login(protocol, relay, { username, password });
  if (!result.success) {
    disconnect();
    return 'Soulseek rejected the sign-in.';
  }
  loginForm.hidden = true;
  session.hidden = false;
  return 'Signed in to Soulseek.';
}, 'Could not sign in. Check the relay URL, token, and allowed origin.');

submit($('#soulseek-search'), 'Preparing search request…', async () => {
  const query = $('#soulseek-query').value.trim();
  if (!query) return 'Enter a search query.';
  const [token] = crypto.getRandomValues(new Uint32Array(1));
  relay.send(await protocol.encodeFileSearch(token, query));
  return 'Search request sent. Search results are unavailable.';
}, 'Could not send the search request. Sign in to try again.');

submit($('#soulseek-peer'), 'Looking up peer address…', async () => {
  const username = $('#soulseek-peer-user').value.trim();
  if (!username) return 'Enter a peer username.';
  const peer = await lookupPeer(protocol, relay, username);
  return peer.port ? `${peer.username} · ${peer.ipAddress}:${peer.port}` : 'Peer address unavailable.';
}, 'Could not look up the peer address. Sign in to try again.');

$('#soulseek-disconnect').addEventListener('click', () => {
  disconnect();
  status.textContent = 'Disconnected.';
});

function openSoulseek() {
  if (location.hash === '#soulseek') $('#soulseek').open = true;
}
window.addEventListener('hashchange', openSoulseek);
openSoulseek();
