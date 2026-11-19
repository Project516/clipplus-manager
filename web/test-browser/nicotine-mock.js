// Stands in for @project516/nicotine-plus-wasm in the fixture build.
const fixture = globalThis.nicotineMock = { connections: [], downloads: [], closedByApp: 0 };

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

const tick = () => new Promise((resolve) => setTimeout(resolve, 20));

function aborted(signal) {
  return new Promise((_, reject) => signal?.addEventListener('abort', () => reject(signal.reason), { once: true }));
}

export async function connectClient({ relayURL, relayToken, username, password }) {
  fixture.connections.push({ relayURL, relayToken, username });
  await tick();
  if (password !== 'fixture-password') {
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
      for (const result of results) {
        await tick();
        if (signal?.aborted) return;
        onResult(result);
      }
    },
    async download(user, path, { signal, onProgress } = {}) {
      fixture.downloads.push({ user, path });
      onProgress?.({ state: 'connecting' });
      await tick();
      if (path.endsWith('Missing.mp3')) {
        throw Object.assign(new Error('Download failed'), { name: 'DownloadError', reason: 'File not shared.' });
      }
      onProgress?.({ state: 'queued', position: 2 });
      if (path.endsWith('Slow.flac')) await aborted(signal);
      const bytes = new TextEncoder().encode(`fixture:${path}`);
      onProgress?.({ state: 'transferring', received: bytes.length / 2, total: bytes.length });
      await tick();
      return new Blob([bytes]);
    },
  };
}
