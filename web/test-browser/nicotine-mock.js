// Stands in for @project516/nicotine-plus-wasm in the fixture build.
import songURL from '../test-fixtures/art.mp3?url';

const fixture = globalThis.nicotineMock = { connections: [], downloads: [], browsed: [], closedByApp: 0 };

const song = (path, size, extra = {}) => {
  const name = path.split('\\').pop();
  return { path, name, size, extension: name.split('.').pop().toLowerCase(), ...extra };
};

const results = [
  { username: 'queue-user', freeSlot: false, queueLength: 4, uploadSpeed: 2_000_000, files: [song('Music\\Album\\01 Song.mp3', 5_000_000, { bitrate: 320, duration: 225 })] },
  { username: 'free-user', freeSlot: true, queueLength: 0, uploadSpeed: 500_000, files: [song('Music\\02 Free.mp3', 2_400_000, { bitrate: 192, duration: 61 }), song('Music\\cover.jpg', 90_000)] },
  { username: 'flac-user', freeSlot: true, queueLength: 0, uploadSpeed: 9_000_000, files: [song('Lossless\\03 Slow.flac', 30_000_000), song('Lossless\\notes.txt', 100)] },
  { username: 'gone-user', freeSlot: true, queueLength: 0, uploadSpeed: 100, files: [song('04 Missing.mp3', 3_000_000)] },
];

const peer = (username, files, extra = {}) => ({ username, freeSlot: true, queueLength: 0, uploadSpeed: 1_000_000, files, ...extra });
const tidewater = 'Music\\The Glass Harbor\\Tidewater (2019)';
const folderSongs = ['Lantern.mp3', 'Missing.mp3', 'Reprise.mp3', 'cover.jpg', 'notes.txt', 'reprise.mp3'];
const albumResults = [
  peer('quiet-keeper', [song(`${tidewater}\\Lantern.mp3`, 5_000_000), song(`${tidewater}\\Missing.mp3`, 5_000_000), song(`${tidewater}\\cover.jpg`, 90_000)]),
  peer('salt-marsh', [song('Albums\\Glass Harbor - Tidewater\\01 Slow.flac', 30_000_000), song('Albums\\Glass Harbor - Tidewater\\02 Tide.flac', 30_000_000)], { freeSlot: false, queueLength: 518 }),
  peer('quiet-keeper', [song(`${tidewater}\\Reprise.mp3`, 5_000_000), song(`${tidewater}\\reprise.mp3`, 5_000_000)]),
  peer('harbor-light', [song('Vinyl\\Locked\\Side A.mp3', 5_000_000), song('loose.mp3', 5_000_000)], { uploadSpeed: 3_000_000 }),
];
const folders = new Map([
  ['queue-user\nMusic\\Album', ['01 Song.mp3']],
  [`quiet-keeper\n${tidewater}`, folderSongs],
  ['salt-marsh\nAlbums\\Glass Harbor - Tidewater', ['01 Slow.flac', '02 Tide.flac']],
]);

const tick = () => new Promise((resolve) => setTimeout(resolve, 20));

function aborted(signal) {
  return new Promise((_, reject) => signal?.addEventListener('abort', () => reject(signal.reason), { once: true }));
}

export async function connectClient({ relayURL, relayToken, username, password }) {
  fixture.connections.push({ relayURL, relayToken, username });
  await tick();
  // The browser account has a 48-character hex password.
  if (password !== 'fixture-password' && !/^[0-9a-f]{48}$/.test(password)) {
    throw Object.assign(new Error('Login failed'), { name: 'LoginError', reason: 'INVALIDPASS' });
  }
  const closed = Promise.withResolvers();
  fixture.drop = () => closed.resolve(new Error('Relay closed'));
  return {
    closed: closed.promise,
    close() {
      fixture.closedByApp += 1;
    },
    async search(query, { onResult, signal }) {
      fixture.query = query;
      for (const result of /tidewater/i.test(query) ? albumResults : results) {
        await tick();
        if (signal?.aborted) return;
        onResult(result);
      }
    },
    // Mirrors browseFolder in nicotine-plus-wasm 0.3.0: files directly in the folder, sorted by name.
    async browseFolder(user, folder, { signal } = {}) {
      fixture.browsed.push({ user, folder });
      await tick();
      signal?.throwIfAborted();
      const names = folders.get(`${user}\n${folder}`);
      if (!names) throw Object.assign(new Error('Browse failed'), { name: 'DownloadError', reason: 'The peer did not list the folder in time' });
      return names.map((name) => song(`${folder}\\${name}`, 5_000_000)).sort((a, b) => (a.name < b.name ? -1 : 1));
    },
    async download(user, path, { size, signal, onProgress } = {}) {
      fixture.downloads.push({ user, path, size });
      onProgress?.({ state: 'connecting' });
      await tick();
      if (path.endsWith('Missing.mp3')) {
        throw Object.assign(new Error('Download failed'), { name: 'DownloadError', reason: 'The user is offline' });
      }
      onProgress?.({ state: 'queued', position: 2 });
      if (path.endsWith('Slow.flac')) await aborted(signal);
      const bytes = await (await fetch(songURL)).arrayBuffer();
      onProgress?.({ state: 'transferring', received: bytes.byteLength / 2, total: bytes.byteLength });
      await tick();
      return new Blob([bytes]);
    },
  };
}
