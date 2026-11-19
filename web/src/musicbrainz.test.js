import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { parseRecordings, recordingDetails, searchRecordings, words } from './musicbrainz.js';

const fixture = async () => JSON.parse(await readFile(new URL('../test-fixtures/musicbrainz.json', import.meta.url), 'utf8'));
const id = (n) => `00000000-0000-4000-8000-00000000000${n}`;

test('splits words on anything but letters, digits and apostrophes', () => {
  assert.deepEqual(words(' Toto  AFRICA '), ['toto', 'africa']);
  assert.deepEqual(words('AC/DC & t.n.t'), ['ac', 'dc', 't', 'n', 't']);
  assert.deepEqual(words('Don\u2019t Stop 99 - \''), ["don't", 'stop', '99']);
  assert.deepEqual(words('Beyonc\u00e9'), ['beyonc\u00e9']);
  // A combining accent splits the word, the same as in core/src/musicbrainz.rs.
  assert.deepEqual(words('Cafe\u0301 Band'), ['cafe', 'band']);
  // An alphabetic vowel sign stays in the word.
  assert.deepEqual(words('Band \u0915\u093e'), ['band', '\u0915\u093e']);
  assert.deepEqual(words(' & - '), []);
});

test('keeps one row per song from the best release, closest match first', async () => {
  assert.deepEqual(parseRecordings(await fixture(), 'glass harbor northern lights'), [
    { id: id(1), title: 'Northern Lights', artist: 'The Glass Harbor', release: 'Tidewater', lengthMs: 241000 },
    { id: id(4), title: 'Northern Lights (Live)', artist: 'The Glass Harbor', release: 'Live at the Pier', lengthMs: 287000 },
    { id: id(3), title: 'Lantern', artist: 'The Glass Harbor', release: undefined, lengthMs: undefined },
    { id: id(6), title: 'Salt Air', artist: '', release: undefined, lengthMs: undefined },
    { id: id(2), title: 'Northern Lights', artist: 'Glass Harbor Trio feat. Mira Salt', release: 'Small Hours', lengthMs: 198000 },
  ]);
});

test('ranks by fewest extra words, then score, then releases, then order', () => {
  const albums = (count) => Array.from({ length: count }, (_, n) => ({ title: `Album ${n}`, status: 'Official', date: '2000' }));
  const recording = (key, title, artist, score, releases) => ({
    id: key,
    title,
    score,
    'artist-credit': [{ name: artist }],
    releases,
  });
  const rows = parseRecordings({
    recordings: [
      recording('cover', 'Northern Lights (Glass Harbor Cover)', 'Pier Kids', 100, albums(1)),
      recording('quartet', 'Northern Lights', 'Tidal Quartet', 100, albums(3)),
      recording('demo', 'northern lights', 'the glass harbor', 80, albums(1)),
      recording('choir', 'Northern Lights', 'Salt Choir', 100, albums(5)),
      recording('trio', 'Northern Lights', 'Tidal Trio', 100, albums(3)),
      recording('original', 'Northern Lights', 'The Glass Harbor', 90, [
        { title: 'Greatest Tides', status: 'Official', date: '2022', 'release-group': { 'primary-type': 'Album', 'secondary-types': ['Compilation'] } },
        { title: 'Tidewater', status: 'Official', date: '2019', 'release-group': { 'primary-type': 'Album', 'secondary-types': [] } },
        { title: 'Promo', status: 'Promotion', date: '2018' },
        { title: 'Reissue', status: 'Official', date: '2020' },
      ]),
      recording('faint', 'Northern Lights', 'Mira Salt', 70, albums(9)),
    ],
  }, 'the glass harbor northern lights');
  assert.deepEqual(rows.map((row) => row.id), ['original', 'choir', 'quartet', 'trio', 'faint', 'cover']);
  assert.equal(rows[0].release, 'Tidewater');
});

test('a repeated song keeps its best ranked copy and that copy\'s best release', () => {
  const recording = (n, title, score, releases) => ({
    id: String(n),
    title,
    score,
    'artist-credit': [{ name: 'Band' }],
    releases,
  });
  const album = (title, status, date) => ({ title, status, date, 'release-group': { 'primary-type': 'Album', 'secondary-types': [] } });
  const rows = parseRecordings({
    recordings: [
      recording(1, 'Song', 50, [album('Early', 'Official', '1980')]),
      recording(2, 'SONG', 90, [album('Bootleg', 'Bootleg', '1979'), album('Reissue', 'Official', '2003'), album('Debut', 'Official', '2001')]),
      recording(3, 'song', 90, [album('Other', 'Official', '2000')]),
    ],
  }, 'band song');
  assert.deepEqual(rows.map((row) => [row.id, row.release]), [['2', 'Debut']]);
});

test('rejects an answer without recordings', () => {
  assert.throws(() => parseRecordings({}, 'band'), /no recordings/);
  assert.deepEqual(parseRecordings({ recordings: [] }, 'band'), []);
});

test('describes a recording for the list', async () => {
  const [tidewater, , lantern, saltAir] = parseRecordings(await fixture(), 'glass harbor northern lights');
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

test('sends one dismax request per search and no custom headers', async () => {
  const body = JSON.stringify(await fixture());
  const requests = [];
  const rows = await withFetch(async (url, init) => {
    requests.push({ url: new URL(url), init });
    return new Response(body);
  }, () => searchRecordings(' glass  harbor '));
  assert.equal(rows.length, 5);
  assert.equal(requests.length, 1);
  const { url, init } = requests[0];
  assert.equal(url.origin + url.pathname, 'https://musicbrainz.org/ws/2/recording/');
  assert.equal(url.searchParams.get('query'), 'glass harbor');
  assert.equal(url.searchParams.get('dismax'), 'true');
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

test('retries once when busy, by status or by error body', async () => {
  const good = JSON.stringify({ recordings: [{ id: '1', title: 'Song', score: 1, 'artist-credit': [{ name: 'Band' }] }] });
  for (const busy of [() => new Response('', { status: 503 }), () => new Response('{"error":"rate limit"}')]) {
    let calls = 0;
    const rows = await withFetch(async () => { calls += 1; return calls === 1 ? busy() : new Response(good); },
      () => searchRecordings('band song', { retryDelayMs: 0 }));
    assert.equal(calls, 2);
    assert.equal(rows.length, 1);
  }
});

test('gives up after one retry and does not retry other failures', async () => {
  let calls = 0;
  const failWith = (make) => withFetch(async () => { calls += 1; return make(); }, () => searchRecordings('band song', { retryDelayMs: 0 }));
  await assert.rejects(failWith(() => new Response('', { status: 503 })), /busy, try again/);
  assert.equal(calls, 2);
  calls = 0;
  await assert.rejects(failWith(() => new Response('', { status: 500 })), /MusicBrainz search: HTTP 500/);
  await assert.rejects(failWith(() => new Response('not json')), /MusicBrainz response/);
  await assert.rejects(failWith(() => new Response('{}')), /MusicBrainz response: no recordings/);
  await assert.rejects(failWith(() => { throw new TypeError('Failed to fetch'); }), /MusicBrainz search: Failed to fetch/);
  assert.equal(calls, 4);
});

test('passes an abort through unchanged, also while waiting to retry', async () => {
  const controller = new AbortController();
  controller.abort();
  const reason = controller.signal.reason;
  await assert.rejects(
    withFetch(async (url, { signal }) => { signal.throwIfAborted(); }, () => searchRecordings('band song', { signal: controller.signal })),
    (error) => error === reason,
  );
  const later = new AbortController();
  let calls = 0;
  const pending = withFetch(async () => { calls += 1; return new Response('', { status: 503 }); },
    () => searchRecordings('band song', { signal: later.signal, retryDelayMs: 60_000 }));
  await new Promise((resolve) => setTimeout(resolve, 10));
  later.abort();
  await assert.rejects(pending, (error) => error === later.signal.reason);
  assert.equal(calls, 1);
});
