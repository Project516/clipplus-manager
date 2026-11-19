import assert from 'node:assert/strict';
import { test } from 'node:test';
import { crc32, uniqueName, zipStored } from './zip.js';

test('computes the standard CRC-32', () => {
  assert.equal(crc32(new TextEncoder().encode('123456789')), 0xcbf43926);
  assert.equal(crc32(new Uint8Array()), 0);
});

// Reads the zip the way an extractor does: end record, central directory, then local headers.
function readZip(buffer) {
  const view = new DataView(buffer);
  const bytes = new Uint8Array(buffer);
  const endAt = buffer.byteLength - 22;
  assert.equal(view.getUint32(endAt, true), 0x06054b50, 'end record');
  assert.equal(view.getUint16(endAt + 8, true), view.getUint16(endAt + 10, true));
  assert.equal(view.getUint16(endAt + 20, true), 0, 'no comment');
  const count = view.getUint16(endAt + 10, true);
  const centralSize = view.getUint32(endAt + 12, true);
  let at = view.getUint32(endAt + 16, true);
  assert.equal(at + centralSize, endAt, 'central directory ends at the end record');
  const entries = [];
  for (let i = 0; i < count; i += 1) {
    assert.equal(view.getUint32(at, true), 0x02014b50, 'central header');
    const nameLength = view.getUint16(at + 28, true);
    const entry = {
      method: view.getUint16(at + 10, true),
      crc: view.getUint32(at + 16, true),
      compressed: view.getUint32(at + 20, true),
      size: view.getUint32(at + 24, true),
      name: new TextDecoder().decode(bytes.subarray(at + 46, at + 46 + nameLength)),
    };
    const local = view.getUint32(at + 42, true);
    assert.equal(view.getUint32(local, true), 0x04034b50, 'local header');
    assert.equal(view.getUint16(local + 6, true) & 0x0800, 0x0800, 'UTF-8 flag');
    assert.equal(view.getUint16(local + 8, true), 0, 'stored');
    assert.equal(view.getUint32(local + 14, true), entry.crc);
    assert.equal(view.getUint32(local + 18, true), entry.compressed);
    assert.equal(view.getUint32(local + 22, true), entry.size);
    assert.equal(view.getUint16(local + 26, true), nameLength);
    assert.equal(view.getUint16(local + 28, true), 0, 'no extra field');
    const dataAt = local + 30 + nameLength;
    entry.data = bytes.subarray(dataAt, dataAt + entry.size);
    assert.equal(new TextDecoder().decode(bytes.subarray(local + 30, dataAt)), entry.name);
    entries.push(entry);
    at += 46 + nameLength;
  }
  return entries;
}

test('writes a stored zip that reads back', async () => {
  const songs = [
    { name: '01 Low Tide.mp3', bytes: Uint8Array.from([1, 2, 3, 4, 5]) },
    { name: 'Café ♪.flac', bytes: new Uint8Array(70_000).map((_, i) => i % 251) },
    { name: 'empty.mp3', bytes: new Uint8Array() },
  ];
  const blob = zipStored(songs);
  assert.equal(blob.type, 'application/zip');
  const entries = readZip(await blob.arrayBuffer());
  assert.deepEqual(entries.map((entry) => entry.name), songs.map((song) => song.name));
  entries.forEach((entry, i) => {
    assert.equal(entry.method, 0);
    assert.equal(entry.size, songs[i].bytes.length);
    assert.equal(entry.crc, crc32(songs[i].bytes));
    assert.deepEqual(entry.data, songs[i].bytes);
  });
});

test('writes an empty zip as just the end record', async () => {
  const buffer = await zipStored([]).arrayBuffer();
  assert.equal(buffer.byteLength, 22);
  assert.deepEqual(readZip(buffer), []);
});

test('numbers repeated names without case', () => {
  const taken = new Set();
  assert.equal(uniqueName('Song.mp3', taken), 'Song.mp3');
  assert.equal(uniqueName('song.mp3', taken), 'song (2).mp3');
  assert.equal(uniqueName('SONG.mp3', taken), 'SONG (3).mp3');
  assert.equal(uniqueName('Other.flac', taken), 'Other.flac');
  assert.equal(uniqueName('Song (2).mp3', taken), 'Song (2) (2).mp3');
  assert.equal(uniqueName('noext', taken), 'noext');
  assert.equal(uniqueName('noext', taken), 'noext (2)');
});
