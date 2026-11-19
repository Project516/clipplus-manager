import { offerSave, safeName } from './output.js';
import { RESULT_LIMIT, addResult, describeProgress, fileDetails, fileKey, formatSize, secureRelayURL } from './soulseek.js';

const $ = (selector) => document.querySelector(selector);
const loginForm = $('#soulseek-login');
const searchForm = $('#soulseek-search');
const query = $('#soulseek-query');
const relayURL = $('#relay-url');
const relayAccess = $('#relay-access');
const relayAccessLink = $('#relay-access-link');
const relayURLKey = 'soulseekRelayURL';
const status = $('#soulseek-status');
const session = $('#soulseek-session');
const searching = $('#soulseek-searching');
const files = $('#soulseek-files');
const downloads = $('#soulseek-downloads');
const results = $('#soulseek-results');
const choices = results.querySelector('.choices');
const downloadButton = $('#soulseek-download');

let client;
let search;
let rows = [];
let renderFrame;
let selected;
const choiceItems = new Map();
const transfers = new Map();

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

function updateRelayAccess() {
  try {
    const healthURL = new URL('/healthz', secureRelayURL(relayURL.value));
    healthURL.protocol = 'https:';
    relayAccessLink.href = healthURL.href;
    relayAccess.hidden = false;
  } catch {
    relayAccessLink.removeAttribute('href');
    relayAccess.hidden = true;
  }
}

try {
  relayURL.value = localStorage.getItem(relayURLKey) || '';
} catch { /* Storage can be disabled. */ }
updateRelayAccess();
relayURL.addEventListener('input', updateRelayAccess);
relayURL.addEventListener('change', () => {
  try {
    if (relayURL.value) {
      localStorage.setItem(relayURLKey, secureRelayURL(relayURL.value).href);
    } else {
      localStorage.removeItem(relayURLKey);
    }
  } catch { /* Invalid URLs and disabled storage cannot be saved. */ }
});

function showSession(signedIn) {
  loginForm.hidden = signedIn;
  session.hidden = !signedIn;
  files.hidden = !signedIn;
}

function signOut(message) {
  const closing = client;
  client = undefined;
  search?.abort();
  search = undefined;
  searching.hidden = true;
  for (const controller of transfers.values()) controller.abort();
  transfers.clear();
  closing?.close();
  rows = [];
  renderResults();
  showSession(false);
  status.textContent = message;
}

function scheduleRender() {
  renderFrame ??= requestAnimationFrame(renderResults);
}

function updateDownloadButton() {
  downloadButton.disabled = !client || !selected || transfers.has(fileKey(selected));
}

// Moves only the rows whose place changed, so the focused row keeps focus.
function renderResults() {
  cancelAnimationFrame(renderFrame);
  renderFrame = undefined;
  const keys = new Set(rows.map(fileKey));
  for (const [key, item] of choiceItems) {
    if (!keys.has(key)) {
      item.remove();
      choiceItems.delete(key);
    }
  }
  if (selected && !keys.has(fileKey(selected))) selected = undefined;
  rows.forEach((file, index) => {
    const key = fileKey(file);
    let item = choiceItems.get(key);
    if (!item) {
      item = choiceItem(file);
      choiceItems.set(key, item);
    }
    if (choices.children[index] !== item) choices.insertBefore(item, choices.children[index] ?? null);
  });
  results.hidden = !rows.length;
  updateDownloadButton();
}

function fileText(file, details, tag = 'span') {
  const text = element(tag, 'file');
  const name = element('span', 'file-name', file.name);
  name.title = file.path;
  text.append(name, element('span', 'file-details', details));
  return text;
}

function choiceItem(file) {
  const item = element('label');
  const input = element('input');
  input.type = 'radio';
  input.name = 'soulseek-file';
  input.addEventListener('change', () => {
    selected = file;
    updateDownloadButton();
  });
  item.append(input, fileText(file, fileDetails(file)));
  item.addEventListener('dblclick', () => download(file));
  return item;
}

function setMeter(meter, fraction) {
  meter.hidden = fraction === undefined;
  meter.classList.toggle('busy', fraction === null);
  const value = Math.round((fraction ?? 0) * 100);
  meter.firstChild.style.width = fraction === null ? '' : `${value}%`;
  if (fraction == null) meter.removeAttribute('aria-valuenow');
  else meter.setAttribute('aria-valuenow', value);
}

async function download(file) {
  const key = fileKey(file);
  if (!client || transfers.has(key)) return;
  const controller = new AbortController();
  transfers.set(key, controller);
  updateDownloadButton();

  const item = element('li');
  const text = fileText(file, '', 'div');
  const detail = text.lastChild;
  const meter = element('div', 'meter');
  meter.setAttribute('role', 'progressbar');
  meter.setAttribute('aria-label', `${file.name} progress`);
  meter.append(element('span'));
  text.append(meter);
  const cancel = element('button', '', 'Cancel');
  cancel.type = 'button';
  cancel.setAttribute('aria-label', `Cancel ${file.name}`);
  cancel.addEventListener('click', () => controller.abort());
  const save = element('a', 'save', 'Save');
  save.hidden = true;
  save.setAttribute('aria-label', `Save ${file.name}`);
  item.append(text, cancel, save);
  downloads.querySelector('ul').prepend(item);
  downloads.hidden = false;
  status.textContent = `Downloading ${file.name}.`;

  const show = ({ text: message, fraction }, stage = '') => {
    detail.textContent = message;
    detail.className = `file-details ${stage}`.trim();
    setMeter(meter, fraction);
  };
  show(describeProgress({ state: 'connecting' }));
  try {
    const blob = await client.download(file.username, file.path, {
      size: file.size,
      signal: controller.signal,
      onProgress: (progress) => {
        if (!controller.signal.aborted) show(describeProgress(progress));
      },
    });
    controller.signal.throwIfAborted();
    offerSave(save, blob, safeName(file.name, file.extension));
    show({ text: `Ready to save · ${formatSize(blob.size)}` }, 'done');
    status.textContent = `${file.name} is ready to save.`;
  } catch (error) {
    const message = controller.signal.aborted ? 'Canceled' : `Download failed: ${error?.reason || 'the user could not send the file.'}`;
    show({ text: message }, controller.signal.aborted ? '' : 'failed');
    status.textContent = `${file.name}: ${message}`;
  } finally {
    cancel.remove();
    if (transfers.get(key) === controller) transfers.delete(key);
    updateDownloadButton();
  }
}

loginForm.addEventListener('submit', async (event) => {
  event.preventDefault();
  if (client) return;
  let url;
  try {
    url = secureRelayURL(relayURL.value);
  } catch {
    status.textContent = 'Enter a wss:// relay URL. Put the relay token in its own field.';
    return;
  }
  const relayToken = $('#relay-token').value;
  const username = $('#soulseek-user').value;
  const password = $('#soulseek-password').value;
  $('#relay-token').value = '';
  $('#soulseek-password').value = '';
  const button = loginForm.querySelector('button');
  button.disabled = true;
  status.textContent = 'Signing in…';
  try {
    const { connectClient } = await import('@project516/nicotine-plus-wasm');
    const connected = await connectClient({ relayURL: url.href, relayToken, username, password });
    client = connected;
    const dropped = () => {
      if (client === connected) signOut('Relay disconnected. Sign in to reconnect.');
    };
    connected.closed.then(dropped, dropped);
    showSession(true);
    status.textContent = 'Signed in to Soulseek.';
    query.focus();
  } catch (error) {
    status.textContent = error?.reason
      ? `Soulseek rejected the sign-in: ${error.reason}`
      : 'Could not sign in. Check the relay URL, token, and allowed origin. If your relay uses Cloudflare Access, sign in to the relay first.';
  } finally {
    button.disabled = false;
  }
});

searchForm.addEventListener('submit', async (event) => {
  event.preventDefault();
  const text = query.value.trim();
  if (!client || !text) return;
  search?.abort();
  const controller = new AbortController();
  search = controller;
  rows = [];
  renderResults();
  searching.hidden = false;
  status.textContent = `Searching for ${text}…`;
  try {
    await client.search(text, {
      signal: controller.signal,
      onResult: (result) => {
        if (search !== controller) return;
        rows = addResult(rows, result);
        scheduleRender();
      },
    });
  } catch {
    if (search === controller) status.textContent = 'The search stopped early. Try again.';
    return;
  } finally {
    if (search === controller) {
      search = undefined;
      searching.hidden = true;
    }
  }
  if (controller.signal.aborted) return;
  renderResults();
  if (!rows.length) status.textContent = 'No playable songs found. Try other words.';
  else if (rows.length === RESULT_LIMIT) status.textContent = `Showing the best ${RESULT_LIMIT} files.`;
  else status.textContent = `Found ${rows.length} ${rows.length === 1 ? 'file' : 'files'}.`;
});

downloadButton.addEventListener('click', () => {
  if (selected) download(selected);
});
$('#soulseek-disconnect').addEventListener('click', () => signOut('Disconnected.'));
