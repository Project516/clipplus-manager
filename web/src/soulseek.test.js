import assert from 'node:assert/strict';
import { test } from 'node:test';
import { addResult, albumDetails, baseName, describeProgress, fileDetails, formatDuration, formatSize, groupAlbumFiles, parentFolder, plural, secureRelayURL } from './soulseek.js';

test('accepts only secure relay URLs without embedded credentials', () => {
  assert.equal(secureRelayURL('wss://relay.invalid/socket').pathname, '/socket');
  for (const value of ['ws://relay.invalid', 'wss://user@relay.invalid', 'wss://relay.invalid/?key=fixture']) {
    assert.throws(() => secureRelayURL(value));
  }
});

const file = (name, extra = {}) => ({ path: `Music\\${name}`, name, size: 1, extension: name.split('.').pop(), ...extra });
const peer = (username, freeSlot, uploadSpeed, files) => ({ username, freeSlot, queueLength: 0, uploadSpeed, files });

test('keeps playable files, mp3 first, then free slots, then upload speed', () => {
  let rows = addResult([], peer('busy', false, 9, [file('a.mp3'), file('cover.jpg'), file('b.flac')]));
  rows = addResult(rows, peer('free', true, 1, [file('c.ogg'), file('d.mp3'), file('e.wav')]));
  rows = addResult(rows, peer('fast', true, 5, [file('f.mp3'), file('g.opus'), file('h.m4a')]));
  assert.deepEqual(rows.map((row) => `${row.username}/${row.name}`), [
    'fast/f.mp3', 'free/d.mp3', 'busy/a.mp3', 'fast/g.opus', 'fast/h.m4a', 'free/c.ogg', 'busy/b.flac',
  ]);
});

test('skips a file the same user already sent', () => {
  const rows = addResult([], peer('peer', true, 1, [file('a.mp3'), file('a.mp3')]));
  assert.equal(addResult(rows, peer('peer', true, 1, [file('a.mp3')])).length, 1);
  assert.equal(addResult(rows, peer('other', true, 1, [file('a.mp3')])).length, 2);
});

test('caps the list at the best results', () => {
  const many = Array.from({ length: 5 }, (_, i) => file(`${i}.flac`));
  let rows = addResult([], peer('slow', false, 1, many), 3);
  rows = addResult(rows, peer('quick', true, 1, [file('x.mp3')]), 3);
  assert.deepEqual(rows.map((row) => row.name), ['x.mp3', '0.flac', '1.flac']);
});

const found = (username, ...paths) => ({ username, files: paths.map((path) => ({ path, name: path.split(/[\\/]/).pop(), size: 1 })) });

test('groups audio files into album folders per user', () => {
  const groups = groupAlbumFiles(new Map(), found('peer',
    'Artist\\Album\\01.flac', 'Artist\\Album\\02.MP3', 'Artist\\Album\\cover.jpg', 'Artist\\Album\\notes.txt', 'loose.mp3', '\\root.mp3'));
  groupAlbumFiles(groups, found('other', 'Artist\\Album\\01.flac'));
  groupAlbumFiles(groups, found('peer', 'Artist/Other/a.ogg', 'Artist/Other/b.wav', 'Artist\\Album\\03.mp3'));
  assert.deepEqual([...groups.values()].map(({ username, folder, name, songs, format }) => ({ username, folder, name, songs, format })), [
    { username: 'peer', folder: 'Artist\\Album', name: 'Album', songs: 3, format: 'mp3' },
    { username: 'other', folder: 'Artist\\Album', name: 'Album', songs: 1, format: 'flac' },
    { username: 'peer', folder: 'Artist/Other', name: 'Other', songs: 2, format: 'ogg' },
  ]);
});

test('names the most common format and counts a repeated file once', () => {
  const groups = groupAlbumFiles(new Map(), found('peer', 'A\\B\\1.flac', 'A\\B\\2.mp3', 'A\\B\\3.mp3', 'A\\B\\3.mp3'));
  assert.deepEqual([...groups.values()].map(albumDetails), ['peer · 3 songs · mp3']);
  assert.equal(albumDetails({ username: 'peer', songs: 1, format: 'ogg' }), 'peer · 1 song · ogg');
});

test('caps album folders but still counts songs in the ones kept', () => {
  const groups = groupAlbumFiles(new Map(), found('peer', 'A\\1\\a.mp3', 'A\\2\\a.mp3', 'A\\3\\a.mp3', 'A\\1\\b.mp3'), 2);
  assert.deepEqual([...groups.values()].map((group) => [group.folder, group.songs]), [['A\\1', 2], ['A\\2', 1]]);
});

test('splits a path into folder and name', () => {
  assert.equal(parentFolder('Music\\Album\\01 Song.mp3'), 'Music\\Album');
  assert.equal(parentFolder('Music/Album/01 Song.mp3'), 'Music/Album');
  assert.equal(parentFolder('Song.mp3'), undefined);
  assert.equal(parentFolder('\\Song.mp3'), undefined);
  assert.equal(baseName('Music\\Album'), 'Album');
  assert.equal(baseName('Album'), 'Album');
  assert.equal(plural(1, 'song'), '1 song');
  assert.equal(plural(0, 'song'), '0 songs');
});

test('describes files and progress for the panel', () => {
  assert.equal(formatSize(8_400_000), '8.4 MB');
  assert.equal(formatDuration(225.4), '3:45');
  assert.equal(formatDuration(59.6), '1:00');
  assert.equal(fileDetails({ username: 'peer', size: 5e6, bitrate: 320, duration: 61 }), 'peer · 5.0 MB · 320 kbps · 1:01');
  assert.equal(fileDetails({ username: 'peer', size: 5e6 }), 'peer · 5.0 MB');
  assert.deepEqual(describeProgress({ state: 'connecting' }), { text: 'Connecting to the user', fraction: null });
  assert.deepEqual(describeProgress({ state: 'queued', position: 3 }), { text: 'Queued, position 3', fraction: null });
  assert.deepEqual(describeProgress({ state: 'queued' }), { text: 'Queued by the user', fraction: null });
  assert.deepEqual(describeProgress({ state: 'transferring', received: 1, total: 4e6 }), { text: '0% of 4.0 MB', fraction: 1 / 4e6 });
  assert.deepEqual(describeProgress({ state: 'transferring', received: 3e6, total: 4e6 }), { text: '75% of 4.0 MB', fraction: 0.75 });
});
