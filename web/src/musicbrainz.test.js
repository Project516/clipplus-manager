import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { lucene, parseRecordings, recordingDetails, searchRecordings } from './musicbrainz.js';

const fixture = async () => JSON.parse(await readFile(new URL('../test-fixtures/musicbrainz.json', import.meta.url), 'utf8'));
const id = (n) => `00000000-0000-4000-8000-00000000000${n}`;

test('each word matches artist or title', () => {
  assert.equal(lucene(' toto  africa '), '(artist:toto OR recording:toto) AND (artist:africa OR recording:africa)');
  assert.equal(lucene('AC/DC & t.n.t'), '(artist:AC\\/DC OR recording:AC\\/DC) AND (artist:t.n.t OR recording:t.n.t)');
  assert.equal(lucene(' & - '), '');
  assert.equal(lucene('  '), '');
  assert.equal(lucene('Beyoncé 99'), '(artist:Beyoncé OR recording:Beyoncé) AND (artist:99 OR recording:99)');
});

test('keeps one row per song from the best release', async () => {
  assert.deepEqual(parseRecordings(await fixture()), [
    { id: id(1), title: 'Northern Lights', artist: 'The Glass Harbor', release: 'Tidewater', lengthMs: 241000 },
    { id: id(2), title: 'Northern Lights', artist: 'Glass Harbor Trio feat. Mira Salt', release: 'Small Hours', lengthMs: 198000 },
    { id: id(4), title: 'Northern Lights (Live)', artist: 'The Glass Harbor', release: 'Live at the Pier', lengthMs: 287000 },
    { id: id(3), title: 'Lantern', artist: 'The Glass Harbor', release: undefined, lengthMs: undefined },
    { id: id(6), title: 'Salt Air', artist: '', release: undefined, lengthMs: undefined },
  ]);
});

test('a later copy replaces an earlier one only on a better release', () => {
  const recording = (n, title, status, date) => ({
    id: String(n),
    title,
    'artist-credit': [{ name: 'Band' }],
    releases: [{ title: `Release ${n}`, status, date, 'release-group': { 'primary-type': 'Album', 'secondary-types': [] } }],
  });
  const rows = parseRecordings({
    recordings: [
      recording(1, 'Song', 'Bootleg', '1980'),
      recording(2, 'SONG', 'Official', '2003'),
      recording(3, 'song', 'Official', '2001'),
      recording(4, 'song', 'Official', '2001'),
    ],
  });
  assert.deepEqual(rows.map((row) => [row.id, row.release]), [['3', 'Release 3']]);
});

test('rejects an answer without recordings', () => {
  assert.throws(() => parseRecordings({}), /no recordings/);
  assert.deepEqual(parseRecordings({ recordings: [] }), []);
});

test('describes a recording for the list', async () => {
  const [tidewater, , , lantern, saltAir] = parseRecordings(await fixture());
  assert.equal(recordingDetails(tidewater), 'The Glass Harbor · Tidewater · 4:01');
  assert.equal(recordingDetails(lantern), 'The Glass Harbor');
  assert.equal(recordingDetails(saltAir), '');
});

async function withFetch(replacement, run) {
  const original = globalThis.fetch;
  globalThis.fetch = replacement;
  try {
    return await run();
  } finally {
    globalThis.fetch = original;
  }
}

test('sends one JSON request per search and no custom headers', async () => {
  const body = JSON.stringify(await fixture());
  const requests = [];
  const rows = await withFetch(async (url, init) => {
    requests.push({ url: new URL(url), init });
    return new Response(body);
  }, () => searchRecordings(' glass harbor '));
  assert.equal(rows.length, 5);
  assert.equal(requests.length, 1);
  const { url, init } = requests[0];
  assert.equal(url.origin + url.pathname, 'https://musicbrainz.org/ws/2/recording/');
  assert.equal(url.searchParams.get('query'), '(artist:glass OR recording:glass) AND (artist:harbor OR recording:harbor)');
  assert.equal(url.searchParams.get('fmt'), 'json');
  assert.equal(url.searchParams.get('limit'), '100');
  assert.equal(init.headers, undefined);
});

test('does not search for an empty query', async () => {
  let calls = 0;
  const rows = await withFetch(async () => { calls += 1; }, () => searchRecordings(' & - '));
  assert.deepEqual(rows, []);
  assert.equal(calls, 0);
});

test('explains a busy or broken service without retrying', async () => {
  let calls = 0;
  const failWith = (make) => withFetch(async () => { calls += 1; return make(); }, () => searchRecordings('band song'));
  await assert.rejects(failWith(() => new Response('', { status: 503 })), /busy, try again/);
  await assert.rejects(failWith(() => new Response('', { status: 500 })), /MusicBrainz search: HTTP 500/);
  await assert.rejects(failWith(() => new Response('not json')), /MusicBrainz response/);
  await assert.rejects(failWith(() => { throw new TypeError('Failed to fetch'); }), /MusicBrainz search: Failed to fetch/);
  assert.equal(calls, 4);
});

test('passes an abort through unchanged', async () => {
  const controller = new AbortController();
  controller.abort();
  const reason = controller.signal.reason;
  await assert.rejects(
    withFetch(async (url, { signal }) => { signal.throwIfAborted(); }, () => searchRecordings('band song', { signal: controller.signal })),
    (error) => error === reason,
  );
});
