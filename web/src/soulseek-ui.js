import { createJobs, element } from './dom.js';
import { FFmpeg, ffmpegCore } from './ffmpeg.js';
import { fixForSansa } from './fixer.js';
import { recordingDetails, searchRecordings } from './musicbrainz.js';
import { safeName } from './output.js';
import {
  PLAYABLE, RESULT_LIMIT, addResult, albumDetails, albumKey, baseName, describeProgress, fileDetails, fileKey,
  formatSize, groupAlbumFiles, parentFolder, plural, secureRelayURL,
} from './soulseek.js';
import { uniqueName, zipStored } from './zip.js';

const $ = (selector) => document.querySelector(selector);
const loginForm = $('#soulseek-login');
const searchForm = $('#soulseek-search');
const mode = $('#soulseek-mode');
const query = $('#soulseek-query');
const queryLabel = $('#soulseek-query-label');
const searchButton = searchForm.querySelector('button');
const relayURL = $('#relay-url');
const relayAccess = $('#relay-access');
const relayAccessLink = $('#relay-access-link');
const relayURLKey = 'soulseekRelayURL';
const status = $('#soulseek-status');
const session = $('#soulseek-session');
const lookupMeter = $('#soulseek-searching');
const peerMeter = $('#soulseek-peer-searching');
const pick = $('#soulseek-pick');
const songsSection = $('#soulseek-songs');
const songChoices = songsSection.querySelector('.choices');
const findButton = $('#soulseek-find');
const empty = $('#soulseek-empty');
const results = $('#soulseek-results');
const resultsHeading = $('#soulseek-results-heading');
const track = $('#soulseek-track');
const choices = results.querySelector('.choices');
const backButton = $('#soulseek-back');
const downloadButton = $('#soulseek-download');
const fullAlbumButton = $('#soulseek-full-album');
const addJob = createJobs($('#soulseek-downloads'), $('#soulseek-clear'));

const hints = {
  song: ['Search MusicBrainz for a song', 'Then pick a file from Soulseek. It is fixed for your Sansa before you save it.'],
  album: ['Search Soulseek for an album', 'Choose a folder. Its songs are fixed for your Sansa and saved as one .zip.'],
};

let client;
let search;
let step = 'search';
let songs = [];
let selectedSong;
let fileSong;
let rows = [];
let selected;
let renderFrame;
const choiceItems = new Map();
const transfers = new Map();
let fixing = Promise.resolve();

const isFolder = (row) => 'folder' in row;
const rowKey = (row) => (isFolder(row) ? albumKey(row.username, row.folder) : fileKey(row));
const rowDetails = (row) => (isFolder(row) ? albumDetails(row) : fileDetails(row));

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

function showView() {
  const signedIn = Boolean(client);
  const picking = signedIn && step === 'search';
  loginForm.hidden = signedIn;
  session.hidden = !picking;
  pick.hidden = !picking;
  results.hidden = !signedIn || step !== 'files';
}

function endSearch() {
  search = undefined;
  lookupMeter.hidden = true;
  peerMeter.hidden = true;
  searchButton.disabled = false;
}

function stopSearch() {
  search?.abort();
  endSearch();
}

function startSearch() {
  stopSearch();
  search = new AbortController();
  return search;
}

function showStep(next, heading, trackText = '') {
  step = next;
  stopSearch();
  rows = [];
  selected = undefined;
  renderResults();
  if (next === 'files') {
    const album = mode.value === 'album';
    resultsHeading.textContent = heading;
    track.textContent = trackText;
    fullAlbumButton.hidden = album;
    downloadButton.textContent = album ? 'Download album' : 'Download';
  }
  showView();
}

function showEmpty(title, text = '') {
  empty.hidden = false;
  empty.firstChild.textContent = title;
  empty.lastChild.textContent = text;
}

function renderSongs() {
  selectedSong = undefined;
  findButton.disabled = true;
  songChoices.replaceChildren(...songs.map(songItem));
  songsSection.hidden = !songs.length;
  empty.hidden = songs.length > 0;
}

// Back to a blank Find music page for the current mode.
function resetFind() {
  stopSearch();
  songs = [];
  renderSongs();
  const album = mode.value === 'album';
  queryLabel.textContent = album ? 'Artist and album' : 'Artist and song';
  query.placeholder = queryLabel.textContent;
  showEmpty(...hints[mode.value]);
  status.textContent = '';
}

function signOut(message) {
  const closing = client;
  client = undefined;
  for (const controller of transfers.values()) controller.abort();
  transfers.clear();
  closing?.close();
  resetFind();
  showStep('search');
  status.textContent = message;
}

function scheduleRender() {
  renderFrame ??= requestAnimationFrame(renderResults);
}

function updateButtons() {
  const ready = Boolean(client && selected);
  downloadButton.disabled = !ready || transfers.has(rowKey(selected));
  const folder = selected && !isFolder(selected) && parentFolder(selected.path);
  fullAlbumButton.disabled = !ready || !folder || transfers.has(albumKey(selected.username, folder));
}

function choiceText(name, path, details) {
  const text = element('span', 'file');
  const label = element('span', 'file-name', name);
  if (path) label.title = path;
  text.append(label, element('span', 'file-details', details));
  return text;
}

function radioItem(group, onPick, text) {
  const item = element('label');
  const input = element('input');
  input.type = 'radio';
  input.name = group;
  input.addEventListener('change', onPick);
  item.append(input, text);
  return item;
}

function songItem(song) {
  const item = radioItem('soulseek-song', () => {
    selectedSong = song;
    findButton.disabled = !client;
  }, choiceText(song.title, '', recordingDetails(song)));
  item.addEventListener('dblclick', () => findFiles(song));
  return item;
}

function rowItem(row) {
  const item = radioItem('soulseek-file', () => {
    selected = row;
    updateButtons();
  }, choiceText(row.name, row.folder ?? row.path, rowDetails(row)));
  item.addEventListener('dblclick', () => startDownload(row));
  return item;
}

// Moves only the rows whose place changed, so the focused row keeps focus.
function renderResults() {
  cancelAnimationFrame(renderFrame);
  renderFrame = undefined;
  const keys = new Set(rows.map(rowKey));
  for (const [key, item] of choiceItems) {
    if (!keys.has(key)) {
      item.remove();
      choiceItems.delete(key);
    }
  }
  if (selected && !keys.has(rowKey(selected))) selected = undefined;
  rows.forEach((row, index) => {
    const key = rowKey(row);
    let item = choiceItems.get(key);
    if (!item) {
      item = rowItem(row);
      choiceItems.set(key, item);
    }
    const details = item.querySelector('.file-details');
    if (details.textContent !== rowDetails(row)) details.textContent = rowDetails(row);
    if (choices.children[index] !== item) choices.insertBefore(item, choices.children[index] ?? null);
  });
  updateButtons();
}

// One at a time, so several finished downloads do not each start an FFmpeg worker.
function fixQueued(blob, extension, signal, onProgress, metadata) {
  const run = async () => {
    signal.throwIfAborted();
    onProgress(null);
    const ffmpeg = new FFmpeg();
    const stop = () => ffmpeg.terminate();
    signal.addEventListener('abort', stop);
    try {
      ffmpeg.on('progress', ({ progress }) => {
        if (progress >= 0 && progress <= 1) onProgress(progress);
      });
      await ffmpeg.load(ffmpegCore);
      return await fixForSansa(ffmpeg, new Uint8Array(await blob.arrayBuffer()), extension, metadata);
    } finally {
      signal.removeEventListener('abort', stop);
      ffmpeg.terminate();
    }
  };
  const result = fixing.then(run, run);
  fixing = result.then(() => {}, () => {});
  const aborted = new Promise((_, reject) => signal.addEventListener('abort', () => reject(signal.reason), { once: true }));
  aborted.catch(() => {});
  return Promise.race([result, aborted]);
}

// Downloads one song and fixes it for the Sansa. Failures carry the sentence to show.
// report gets the progress and the phase, 'download' or 'fix'.
async function fetchFixed(connection, username, file, signal, report, metadata) {
  report(describeProgress({ state: 'connecting' }), 'download');
  let blob;
  try {
    blob = await connection.download(username, file.path, {
      size: file.size,
      signal,
      onProgress: (progress) => {
        if (!signal.aborted) report(describeProgress(progress), 'download');
      },
    });
  } catch (error) {
    throw signal.aborted ? error : new Error(`Download failed: ${error?.reason || 'the user could not send the file.'}`);
  }
  signal.throwIfAborted();
  report({ text: 'Waiting to fix for the Sansa', fraction: null }, 'fix');
  try {
    return await fixQueued(blob, file.extension, signal, (fraction) => report({ text: 'Fixing for the Sansa', fraction }, 'fix'), metadata);
  } catch (error) {
    throw signal.aborted ? error : new Error(`Could not fix for the Sansa: ${error?.message ?? error}`);
  }
}

async function download(file, metadata) {
  const key = fileKey(file);
  const controller = new AbortController();
  const { signal } = controller;
  transfers.set(key, controller);
  const job = addJob({ name: file.name, path: file.path, onCancel: () => controller.abort() });
  status.textContent = `Downloading ${file.name}.`;
  try {
    const fixed = await fetchFixed(client, file.username, file, signal, (progress) => job.show(progress), metadata);
    signal.throwIfAborted();
    const name = safeName(file.name, fixed.ext);
    job.offer(new Blob([fixed.bytes], { type: fixed.ext === 'mp3' ? 'audio/mpeg' : 'audio/flac' }), name);
    job.show({ text: `Ready to save · ${formatSize(fixed.bytes.length)}` }, 'done');
    status.textContent = `${name} is ready to save.`;
  } catch (error) {
    if (signal.aborted) {
      job.show({ text: 'Canceled' });
    } else {
      job.show({ text: error.message }, 'failed');
      status.textContent = `${file.name}: ${error.message}`;
    }
  } finally {
    job.finish();
    if (transfers.get(key) === controller) transfers.delete(key);
    updateButtons();
  }
}

// An album is one row: songs download and fix one by one, then they are zipped.
async function downloadAlbum(username, folder) {
  const key = albumKey(username, folder);
  const title = baseName(folder);
  const connection = client;
  const controller = new AbortController();
  const { signal } = controller;
  transfers.set(key, controller);
  const job = addJob({ name: title, path: folder, onCancel: () => controller.abort() });
  const list = element('ul', 'tracks');
  job.meter.before(list);
  status.textContent = `Downloading ${title}.`;
  const ready = [];
  const lines = [];
  let songFiles = [];
  let total = 0;
  let failed = 0;
  try {
    job.show({ text: 'Listing the folder', fraction: null });
    const listed = await connection.browseFolder(username, folder, { signal }).catch((error) => {
      throw signal.aborted ? error : new Error(`Could not list the folder: ${error?.reason || 'the user did not answer.'}`);
    });
    songFiles = listed.filter((file) => PLAYABLE.includes(file.extension));
    if (!songFiles.length) throw new Error('No playable songs in this folder.');
    total = songFiles.length;
    for (const file of songFiles) {
      lines.push(list.appendChild(element('li', '', `${file.name} · waiting`)));
    }
    for (const [index, file] of songFiles.entries()) {
      signal.throwIfAborted();
      const report = ({ text: message, fraction }, phase) => {
        lines[index].textContent = `${file.name} · ${message}`;
        const song = phase === 'fix' ? 1 : fraction ?? 0;
        job.show({ text: `${ready.length} of ${total} songs ready`, fraction: (ready.length + song) / total });
      };
      try {
        const fixed = await fetchFixed(connection, username, file, signal, report);
        ready.push({ name: safeName(file.name, fixed.ext), bytes: fixed.bytes });
        lines[index].textContent = `${file.name} · ready`;
        lines[index].className = 'ready';
      } catch (error) {
        if (signal.aborted) throw error;
        failed += 1;
        lines[index].textContent = `${file.name} · ${error.message}`;
        lines[index].className = 'failed';
      }
    }
    if (!ready.length) throw new Error('Download failed: no songs could be downloaded.');
    const taken = new Set();
    const zipName = safeName(`${title}.zip`, 'zip');
    job.offer(zipStored(ready.map(({ name, bytes }) => ({ name: uniqueName(name, taken), bytes }))), zipName, 'Save .zip');
    const size = ready.reduce((sum, song) => sum + song.bytes.length, 0);
    job.show({ text: `Ready to save · ${plural(ready.length, 'song')} · ${formatSize(size)}${failed ? ` · ${failed} failed` : ''}` }, failed ? 'failed' : 'done');
    list.hidden = !failed;
    status.textContent = `${zipName} is ready to save.`;
  } catch (error) {
    if (signal.aborted) {
      lines.forEach((line, index) => {
        if (!line.className) line.textContent = `${songFiles[index].name} · canceled`;
      });
      job.show({ text: total ? `Canceled · ${ready.length} of ${total} songs were ready` : 'Canceled' });
    } else {
      job.show({ text: error.message }, 'failed');
      status.textContent = `${title}: ${error.message}`;
    }
  } finally {
    job.finish();
    if (transfers.get(key) === controller) transfers.delete(key);
    updateButtons();
  }
}

function startDownload(row) {
  if (!client || transfers.has(rowKey(row))) return;
  if (isFolder(row)) {
    downloadAlbum(row.username, row.folder);
  } else {
    const song = fileSong && { title: fileSong.title, artist: fileSong.artist, album: fileSong.release };
    download(row, song);
  }
  showStep('search');
  query.focus();
}

async function searchSoulseek({ text, heading, trackText, add, summary }) {
  showStep('files', heading, trackText);
  const controller = startSearch();
  peerMeter.hidden = false;
  status.textContent = `Searching for ${text}…`;
  try {
    await client.search(text, {
      signal: controller.signal,
      onResult: (result) => {
        if (search !== controller) return;
        add(result);
        scheduleRender();
      },
    });
  } catch {
    if (search === controller) status.textContent = 'The search stopped early. Try again.';
    return;
  } finally {
    if (search === controller) endSearch();
  }
  if (controller.signal.aborted) return;
  renderResults();
  status.textContent = summary();
}

function findFiles(song) {
  if (!client) return;
  fileSong = song;
  return searchSoulseek({
    text: `${song.artist} ${song.title}`,
    heading: 'Choose a file',
    trackText: `${song.artist} · ${song.title}`,
    add: (result) => { rows = addResult(rows, result); },
    summary: () => {
      if (!rows.length) return 'No playable songs found. Try other words.';
      if (rows.length === RESULT_LIMIT) return `Showing the best ${RESULT_LIMIT} files.`;
      return `Found ${plural(rows.length, 'file')}.`;
    },
  });
}

function findAlbums(text) {
  const groups = new Map();
  fileSong = undefined;
  return searchSoulseek({
    text,
    heading: 'Choose an album folder',
    trackText: text,
    add: (result) => { rows = [...groupAlbumFiles(groups, result).values()]; },
    summary: () => {
      if (!rows.length) return 'No album folders found. Try other words.';
      if (rows.length === RESULT_LIMIT) return `Showing the first ${RESULT_LIMIT} album folders.`;
      return `Found ${plural(rows.length, 'album folder')}.`;
    },
  });
}

async function findSongs(text) {
  const controller = startSearch();
  songs = [];
  renderSongs();
  empty.hidden = true;
  lookupMeter.hidden = false;
  searchButton.disabled = true;
  status.textContent = 'Searching MusicBrainz…';
  try {
    songs = await searchRecordings(text, { signal: controller.signal });
  } catch (error) {
    if (search === controller) status.textContent = error.message;
    return;
  } finally {
    if (search === controller) endSearch();
  }
  if (controller.signal.aborted) return;
  renderSongs();
  if (songs.length) {
    status.textContent = `Found ${plural(songs.length, 'recording')}.`;
  } else {
    showEmpty('No recordings found');
    status.textContent = '';
  }
}

backButton.addEventListener('click', () => {
  showStep('search');
  query.focus();
});
mode.addEventListener('change', resetFind);
findButton.addEventListener('click', () => {
  if (selectedSong) findFiles(selectedSong);
});
downloadButton.addEventListener('click', () => {
  if (selected) startDownload(selected);
});
fullAlbumButton.addEventListener('click', () => {
  const folder = selected && !isFolder(selected) && parentFolder(selected.path);
  if (!folder || !client || transfers.has(albumKey(selected.username, folder))) return;
  downloadAlbum(selected.username, folder);
  showStep('search');
  query.focus();
});

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
    showView();
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

searchForm.addEventListener('submit', (event) => {
  event.preventDefault();
  const text = query.value.trim();
  if (!client || !text) return;
  if (mode.value === 'album') findAlbums(text);
  else findSongs(text);
});

$('#soulseek-disconnect').addEventListener('click', () => signOut('Disconnected.'));
resetFind();
