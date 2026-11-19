import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { chromium } from 'playwright-core';
import { preview } from 'vite';

const musicbrainz = await readFile(new URL('../test-fixtures/musicbrainz.json', import.meta.url), 'utf8');
const relayURL = 'wss://relay.invalid/socket';
const healthURL = 'https://relay.invalid/healthz';

// All network except the preview origin stays aborted. MusicBrainz and the relay health page are fixtures.
async function open(t) {
  const server = await preview({ mode: 'fixture', preview: { host: '127.0.0.1', port: 0 } });
  t.after(() => server.close());
  const browser = await chromium.launch({ channel: 'chrome' });
  t.after(() => browser.close());
  const context = await browser.newContext();
  const page = await context.newPage();
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  const musicbrainzRequests = [];

  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    if (url.origin === origin) return route.continue();
    if (url.href === healthURL) return route.fulfill({ contentType: 'text/plain', body: 'ok' });
    if (url.origin === 'https://musicbrainz.org') {
      musicbrainzRequests.push(url);
      return route.fulfill({ contentType: 'application/json', headers: { 'Access-Control-Allow-Origin': '*' }, body: musicbrainz });
    }
    return route.abort();
  });

  await page.goto(origin);
  const mock = (read) => page.evaluate(read);
  const goTo = (name) => page.getByRole('navigation').getByRole('link', { name }).click();
  const defaults = { url: relayURL, token: 'fixture-token', user: 'fixture-user', password: 'fixture-password' };
  const saveSettings = async (values = {}) => {
    const form = { ...defaults, ...values };
    await goTo('Settings');
    await page.getByLabel('Relay URL').fill(form.url);
    await page.getByLabel('Relay token').fill(form.token);
    await page.getByLabel('Soulseek username').fill(form.user);
    await page.getByLabel('Soulseek password').fill(form.password);
    await page.locator('#relay-form').getByRole('button', { name: 'Save settings' }).click();
    await page.getByText('Settings saved.', { exact: true }).waitFor();
  };
  // The mock loads on the first connect.
  const connections = () => mock(() => globalThis.nicotineMock?.connections ?? []);
  const connected = () => page.getByText('Connected to Soulseek', { exact: true }).waitFor();
  return { page, goTo, mock, saveSettings, connections, connected, musicbrainzRequests };
}

async function signedIn(t) {
  const opened = await open(t);
  await opened.saveSettings();
  await opened.goTo('Soulseek');
  await opened.connected();
  return opened;
}

function id3Frames(bytes) {
  const end = 10 + ((bytes[6] << 21) | (bytes[7] << 14) | (bytes[8] << 7) | bytes[9]);
  const frames = {};
  for (let at = 10; at + 10 <= end && bytes[at] !== 0;) {
    const size = bytes.readUInt32BE(at + 4);
    const body = bytes.subarray(at + 10, at + 10 + size);
    const text = body[0] === 1 ? body.toString('utf16le', 3) : body.toString('latin1', 1);
    frames[bytes.toString('latin1', at, at + 4)] = text.replaceAll('\0', '');
    at += 10 + size;
  }
  return frames;
}

// Reads the entries of a stored zip from its central directory.
function readZip(buffer) {
  const entries = [];
  const count = buffer.readUInt16LE(buffer.length - 12);
  let at = buffer.readUInt32LE(buffer.length - 6);
  for (let i = 0; i < count; i += 1) {
    const nameLength = buffer.readUInt16LE(at + 28);
    const size = buffer.readUInt32LE(at + 24);
    const local = buffer.readUInt32LE(at + 42);
    const start = local + 30 + buffer.readUInt16LE(local + 26);
    entries.push({ name: buffer.toString('utf8', at + 46, at + 46 + nameLength), data: buffer.subarray(start, start + size) });
    at += 46 + nameLength;
  }
  return entries;
}

async function saveAs(page, linkName) {
  const link = page.getByRole('link', { name: linkName });
  await link.waitFor();
  const [saved] = await Promise.all([page.waitForEvent('download'), link.click()]);
  return { name: saved.suggestedFilename(), bytes: await readFile(await saved.path()) };
}

test('connects from saved settings, finds a song, downloads, and saves it with a mock client', { timeout: 120_000 }, async (t) => {
  const { page, goTo, mock, saveSettings, connected, musicbrainzRequests } = await open(t);

  await saveSettings({ password: 'wrong-password' });
  await goTo('Soulseek');
  await page.getByText('Soulseek rejected the sign-in: INVALIDPASS', { exact: true }).waitFor();
  const healthPagePromise = page.waitForEvent('popup');
  await page.getByRole('link', { name: 'sign in to the relay in a new tab' }).click();
  const healthPage = await healthPagePromise;
  await healthPage.getByText('ok', { exact: true }).waitFor();
  assert.equal(healthPage.url(), healthURL);
  await healthPage.close();

  await page.reload();
  assert.equal(await page.getByRole('heading', { name: 'Soulseek', level: 1 }).isVisible(), true);
  await page.getByText('Soulseek rejected the sign-in: INVALIDPASS', { exact: true }).waitFor();
  await saveSettings();
  await goTo('Soulseek');
  await page.getByRole('button', { name: 'Connect', exact: true }).click();
  await connected();
  assert.deepEqual(await mock(() => globalThis.nicotineMock.connections.at(-1)), {
    relayURL,
    relayToken: 'fixture-token',
    username: 'fixture-user',
    password: 'fixture-password',
  });
  await page.getByText('Search MusicBrainz for a song', { exact: true }).waitFor();

  await page.getByLabel('Artist and song').fill('glass harbor northern lights');
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await page.getByText('Found 5 recordings.', { exact: true }).waitFor();
  assert.equal(musicbrainzRequests.length, 1);
  assert.equal(musicbrainzRequests[0].pathname, '/ws/2/recording/');
  assert.equal(musicbrainzRequests[0].searchParams.get('fmt'), 'json');
  assert.equal(musicbrainzRequests[0].searchParams.get('query'), 'glass harbor northern lights');
  assert.equal(musicbrainzRequests[0].searchParams.get('dismax'), 'true');
  const songs = page.locator('#soulseek-songs label');
  assert.deepEqual(await songs.locator('.file-name').allTextContents(), ['Northern Lights', 'Northern Lights (Live)', 'Lantern', 'Salt Air', 'Northern Lights']);
  assert.equal(await songs.first().locator('.file-details').textContent(), 'The Glass Harbor · Tidewater · 4:01');
  const findButton = page.getByRole('button', { name: 'Find downloads' });
  assert.equal(await findButton.isDisabled(), true);

  await songs.first().click();
  await findButton.click();
  await page.getByText('Found 4 files.', { exact: true }).waitFor();
  assert.equal(await mock(() => globalThis.nicotineMock.query), 'The Glass Harbor Northern Lights');
  await page.getByRole('heading', { name: 'Choose a file', level: 2 }).waitFor();
  assert.equal(await page.locator('#soulseek-track').textContent(), 'The Glass Harbor · Northern Lights');
  const rows = page.locator('#soulseek-results label');
  assert.deepEqual(await rows.locator('.file-name').allTextContents(), ['02 Free.mp3', '04 Missing.mp3', '03 Slow.flac', '01 Song.mp3']);
  const row = (name) => rows.filter({ hasText: name });
  assert.equal(await row('01 Song.mp3').locator('.file-details').textContent(), 'queue-user · queue 4 · 5.0 MB · 320 kbps · 3:45');
  assert.equal(await row('02 Free.mp3').locator('.file-details').textContent(), 'free-user · free slot · 2.4 MB · 192 kbps · 1:01');
  const downloadButton = page.getByRole('button', { name: 'Download', exact: true });
  const fullAlbumButton = page.getByRole('button', { name: 'Download full album' });
  assert.equal(await downloadButton.isDisabled(), true);
  assert.equal(await fullAlbumButton.isDisabled(), true);

  await row('01 Song.mp3').click();
  assert.equal(await fullAlbumButton.isDisabled(), false);
  await downloadButton.click();
  await page.getByRole('heading', { name: 'Choose a song' }).waitFor();
  const song = await saveAs(page, 'Save 01 Song.mp3');
  assert.equal(song.name, '01 Song.mp3');
  assert.equal(song.bytes.toString('latin1', 0, 4), 'ID3\x03', 'ID3v2.3 tag');
  assert.equal(song.bytes.includes('APIC'), false, 'cover art removed');
  assert.deepEqual(
    (({ TIT2, TPE1, TALB }) => ({ TIT2, TPE1, TALB }))(id3Frames(song.bytes)),
    { TIT2: 'Northern Lights', TPE1: 'The Glass Harbor', TALB: 'Tidewater' },
  );
  assert.deepEqual(await mock(() => globalThis.nicotineMock.downloads.at(-1)), {
    user: 'queue-user',
    path: 'Music\\Album\\01 Song.mp3',
    size: 5_000_000,
  });

  const findDownloads = async () => {
    await findButton.click();
    await page.getByText('Found 4 files.', { exact: true }).waitFor();
  };

  await findDownloads();
  await row('01 Song.mp3').click();
  await fullAlbumButton.click();
  await page.getByRole('heading', { name: 'Choose a song' }).waitFor();
  const album = await saveAs(page, 'Save Album.zip');
  assert.equal(album.name, 'Album.zip');
  const [first, ...others] = readZip(album.bytes);
  assert.equal(others.length, 0);
  assert.equal(first.name, '01 Song.mp3');
  assert.equal(id3Frames(first.data).TIT2, 'Fixture Tone', 'album songs keep their own tags');
  assert.deepEqual(await mock(() => globalThis.nicotineMock.browsed), [{ user: 'queue-user', folder: 'Music\\Album' }]);

  await findDownloads();
  await row('04 Missing.mp3').click();
  await downloadButton.click();
  await page.getByText('Download failed: The user is offline', { exact: true }).waitFor();

  await findDownloads();
  await row('03 Slow.flac').click();
  await downloadButton.click();
  await page.getByText('Queued, position 2', { exact: true }).waitFor();
  await findDownloads();
  await row('03 Slow.flac').click();
  assert.equal(await downloadButton.isDisabled(), true);
  await page.getByRole('button', { name: 'Back' }).click();
  await page.getByRole('button', { name: 'Cancel 03 Slow.flac' }).click();
  await page.getByText('Canceled', { exact: true }).waitFor();
  await findDownloads();
  await row('03 Slow.flac').click();
  assert.equal(await downloadButton.isDisabled(), false);
  await downloadButton.click();
  await page.getByText('Queued, position 2', { exact: true }).waitFor();

  await page.evaluate(() => globalThis.nicotineMock.drop());
  await page.getByText('Canceled', { exact: true }).nth(1).waitFor();
  assert.equal(await page.locator('#soulseek-status').textContent(), 'Relay disconnected. Connect to try again.');
  await page.getByRole('button', { name: 'Connect', exact: true }).waitFor();
  await page.getByRole('link', { name: 'Save 01 Song.mp3' }).waitFor();
  await page.getByRole('button', { name: 'Clear finished' }).click();
  await page.getByRole('heading', { name: 'Downloads', level: 2 }).waitFor({ state: 'hidden' });
  assert.equal(musicbrainzRequests.length, 1, 'one MusicBrainz request for one search');

  await page.getByRole('button', { name: 'Connect', exact: true }).click();
  await connected();
  await page.getByRole('button', { name: 'Disconnect' }).click();
  await page.getByText('Disconnected.', { exact: true }).waitFor();
  assert.ok(await page.evaluate(() => globalThis.nicotineMock.closedByApp) >= 2);
});

test('finds an album folder, saves its songs as a zip, and cancels the rest', { timeout: 120_000 }, async (t) => {
  const { page, mock, musicbrainzRequests } = await signedIn(t);

  await page.getByLabel('Search for').selectOption('Album');
  await page.getByText('Search Soulseek for an album', { exact: true }).waitFor();
  const query = page.getByLabel('Artist and album');
  assert.equal(await query.getAttribute('placeholder'), 'Artist and album');
  await query.fill('glass harbor tidewater');
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await page.getByText('Found 3 album folders.', { exact: true }).waitFor();
  assert.equal(await mock(() => globalThis.nicotineMock.query), 'glass harbor tidewater');
  assert.equal(musicbrainzRequests.length, 0, 'album search stays on Soulseek');
  await page.getByRole('heading', { name: 'Choose an album folder', level: 2 }).waitFor();
  assert.equal(await page.locator('#soulseek-track').textContent(), 'glass harbor tidewater');
  const rows = page.locator('#soulseek-results label');
  assert.deepEqual(await rows.locator('.file-name').allTextContents(), ['Locked', 'Tidewater (2019)', 'Glass Harbor - Tidewater']);
  assert.deepEqual(await rows.locator('.file-details').allTextContents(), [
    'harbor-light · free slot · 1 song · mp3',
    'quiet-keeper · free slot · 4 songs · mp3',
    'salt-marsh · queue 518 · 2 songs · flac',
  ]);
  const row = (name) => rows.filter({ hasText: name });
  const downloadButton = page.getByRole('button', { name: 'Download album' });
  assert.equal(await page.getByRole('button', { name: 'Download full album' }).isHidden(), true);
  assert.equal(await downloadButton.isDisabled(), true);

  await row('Locked').click();
  await downloadButton.click();
  await page.getByText('Could not list the folder: The peer did not list the folder in time', { exact: true }).waitFor();
  assert.equal(await page.getByRole('link', { name: /^Save/ }).count(), 0);

  const search = page.getByRole('button', { name: 'Search', exact: true });
  await search.click();
  await page.getByText('Found 3 album folders.', { exact: true }).waitFor();
  await row('Tidewater (2019)').click();
  await downloadButton.click();
  await page.getByLabel('Artist and album').waitFor();
  const save = page.getByRole('link', { name: 'Save Tidewater (2019).zip' });
  const zip = await saveAs(page, 'Save Tidewater (2019).zip');
  assert.equal(zip.name, 'Tidewater (2019).zip');
  assert.deepEqual(readZip(zip.bytes).map((entry) => entry.name), ['Lantern.mp3', 'Reprise.mp3', 'reprise (2).mp3']);
  for (const entry of readZip(zip.bytes)) {
    assert.equal(entry.data.toString('latin1', 0, 4), 'ID3\x03', entry.name);
    assert.equal(entry.data.includes('APIC'), false, entry.name);
  }
  const job = page.locator('#soulseek-downloads li').filter({ hasText: 'Tidewater (2019)' });
  assert.match(await job.locator('.file-details').first().textContent(), /^Ready to save · 3 songs · [\d.]+ MB · 1 failed$/);
  assert.deepEqual(await job.locator('.tracks li').allTextContents(), [
    'Lantern.mp3 · ready',
    'Missing.mp3 · Download failed: The user is offline',
    'Reprise.mp3 · ready',
    'reprise.mp3 · ready',
  ]);
  assert.deepEqual(await job.locator('.tracks .failed').allTextContents(), ['Missing.mp3 · Download failed: The user is offline']);
  assert.deepEqual(await mock(() => globalThis.nicotineMock.browsed), [
    { user: 'harbor-light', folder: 'Vinyl\\Locked' },
    { user: 'quiet-keeper', folder: 'Music\\The Glass Harbor\\Tidewater (2019)' },
  ]);
  assert.deepEqual(
    await mock(() => globalThis.nicotineMock.downloads.map((entry) => entry.path.split('\\').pop())),
    ['Lantern.mp3', 'Missing.mp3', 'Reprise.mp3', 'reprise.mp3'],
  );

  const href = await save.getAttribute('href');
  await page.getByRole('button', { name: 'Clear finished' }).click();
  await page.getByRole('heading', { name: 'Downloads', level: 2 }).waitFor({ state: 'hidden' });
  assert.equal(await page.evaluate((url) => fetch(url).then(() => 'readable', () => 'released'), href), 'released');

  await search.click();
  await page.getByText('Found 3 album folders.', { exact: true }).waitFor();
  await row('Glass Harbor - Tidewater').click();
  await downloadButton.click();
  await page.getByText('01 Slow.flac · Queued, position 2', { exact: true }).waitFor();
  await page.getByText('0 of 2 songs ready', { exact: true }).waitFor();
  await page.getByRole('button', { name: 'Cancel Glass Harbor - Tidewater' }).click();
  await page.getByText('Canceled · 0 of 2 songs were ready', { exact: true }).waitFor();
  assert.deepEqual(
    await page.locator('#soulseek-downloads .tracks li').allTextContents(),
    ['01 Slow.flac · canceled', '02 Tide.flac · canceled'],
  );
  assert.equal(await page.getByRole('link', { name: /^Save/ }).count(), 0);
  assert.equal(await mock(() => globalThis.nicotineMock.downloads.some((entry) => entry.path.endsWith('Tide.flac'))), false);
});

test('saved settings keep all four values after a reload', { timeout: 60_000 }, async (t) => {
  const { page, goTo, saveSettings, connections } = await open(t);
  await saveSettings({ user: 'saved-user', password: 'saved-password' });
  await page.reload();
  await goTo('Settings');
  assert.equal(await page.getByLabel('Relay URL').inputValue(), relayURL);
  assert.equal(await page.getByLabel('Relay token').inputValue(), 'fixture-token');
  assert.equal(await page.getByLabel('Soulseek username').inputValue(), 'saved-user');
  assert.equal(await page.getByLabel('Soulseek password').inputValue(), 'saved-password');
  assert.equal((await connections()).length, 0, 'saving does not connect');
});

test('rejects a bad relay URL or half a login when saving', { timeout: 60_000 }, async (t) => {
  const { page, goTo } = await open(t);
  await goTo('Settings');
  const save = page.locator('#relay-form').getByRole('button', { name: 'Save settings' });
  await page.getByLabel('Relay URL').fill('https://relay.invalid/socket');
  await save.click();
  await page.getByText(/^Enter a wss:\/\/ relay URL/).waitFor();
  await page.getByLabel('Relay URL').fill(relayURL);
  await page.getByLabel('Soulseek username').fill('only-a-name');
  await save.click();
  await page.getByText(/^Enter both the Soulseek username and password, or leave both empty/).waitFor();
  assert.equal(await page.evaluate(() => localStorage.getItem('soulseekRelayURL')), null);
});

test('asks for Soulseek settings first and does not connect without a token', { timeout: 60_000 }, async (t) => {
  const { page, goTo, saveSettings, connections, connected } = await open(t);
  await goTo('Soulseek');
  await page.getByText('Set up Soulseek in Settings first.', { exact: true }).waitFor();
  assert.equal(await page.getByRole('button', { name: 'Connect', exact: true }).count(), 0);
  assert.equal(await page.getByRole('link', { name: 'sign in to the relay in a new tab' }).isVisible(), false);

  await saveSettings({ token: '' });
  await goTo('Soulseek');
  await page.getByText('Set up Soulseek in Settings first.', { exact: true }).waitFor();
  assert.equal(await page.getByRole('link', { name: 'sign in to the relay in a new tab' }).isVisible(), true);
  await page.waitForTimeout(300);
  assert.equal((await connections()).length, 0);
  assert.equal(await page.getByRole('button', { name: 'Connect', exact: true }).count(), 0);

  await page.locator('#soulseek-setup').getByRole('link', { name: 'Settings' }).click();
  await page.getByRole('heading', { name: 'Settings', level: 1 }).waitFor();
  await saveSettings();
  await goTo('Soulseek');
  await connected();
  assert.equal((await connections()).length, 1, 'the first visit with full settings connects');
});

test('connects on the first visit, stays disconnected after Disconnect, and connects again on request', { timeout: 60_000 }, async (t) => {
  const { page, goTo, saveSettings, connections, connected } = await open(t);
  await saveSettings();
  await goTo('Soulseek');
  await connected();
  assert.equal((await connections()).length, 1);

  await page.getByRole('button', { name: 'Disconnect' }).click();
  await page.getByText('Disconnected.', { exact: true }).waitFor();
  const connect = page.getByRole('button', { name: 'Connect', exact: true });
  await connect.waitFor();
  await goTo('Downloads');
  await goTo('Soulseek');
  await page.waitForTimeout(300);
  assert.equal((await connections()).length, 1, 'opening the page again does not reconnect');
  assert.equal(await connect.isVisible(), true);

  await connect.click();
  await connected();
  assert.equal((await connections()).length, 2);
});

test('uses a typed Soulseek login as given', { timeout: 60_000 }, async (t) => {
  const { page, goTo, saveSettings, connections, connected } = await open(t);
  await saveSettings({ user: 'typed-user' });
  await goTo('Soulseek');
  await connected();
  await page.getByText('Signed in to Soulseek as typed-user.', { exact: true }).waitFor();
  assert.deepEqual((await connections()).at(-1), {
    relayURL,
    relayToken: 'fixture-token',
    username: 'typed-user',
    password: 'fixture-password',
  });
  await goTo('Settings');
  assert.equal(await page.locator('#browser-account').isHidden(), true);
  assert.equal(await page.evaluate(() => localStorage.getItem('soulseekAccount')), null);
});

test('makes a Soulseek account when the login is left empty, reuses it, and shows its name in Settings', { timeout: 60_000 }, async (t) => {
  const { page, goTo, saveSettings, connections, connected } = await open(t);
  await saveSettings({ user: '', password: '' });
  assert.equal(await page.locator('#browser-account').isHidden(), true);
  await goTo('Soulseek');
  await connected();
  const { username, password } = (await connections()).at(-1);
  assert.match(username, /^clipplus[0-9a-f]{10}$/);
  assert.match(password, /^[0-9a-f]{48}$/);
  await page.getByText(`Signed in to Soulseek as ${username}.`, { exact: true }).waitFor();
  await goTo('Settings');
  await page.getByText(`This browser's account: ${username}`, { exact: true }).waitFor();
  assert.equal(await page.getByLabel('Soulseek username').inputValue(), '');
  assert.equal(await page.getByLabel('Soulseek password').inputValue(), '');

  await page.reload();
  await page.getByText(`This browser's account: ${username}`, { exact: true }).waitFor();
  await goTo('Soulseek');
  await connected();
  assert.deepEqual((await connections()).at(-1).username, username);
  assert.equal((await connections()).at(-1).password, password);
});

test('sends a YouTube download without a proxy to Settings', { timeout: 60_000 }, async (t) => {
  const { page } = await open(t);
  await page.getByRole('navigation').getByRole('link', { name: 'Downloads' }).click();
  await page.getByLabel('YouTube link').fill('https://www.youtube.com/watch?v=fixture');
  await page.getByRole('button', { name: 'Make MP3' }).click();
  await page.getByText('Add your proxy URL to make MP3s.', { exact: true }).waitFor();
  assert.equal(await page.getByRole('heading', { name: 'Settings', level: 1 }).isVisible(), true);
  assert.equal(await page.evaluate(() => document.activeElement?.id), 'proxy');
});
