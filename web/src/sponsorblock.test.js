import { strict as assert } from 'node:assert';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { test } from 'node:test';
import { sponsorBlockAudioArgs } from './sponsorblock.js';

const sponsor = (start_time, end_time) => ({ category: 'sponsor', type: 'skip', start_time, end_time });

test('leaves audio intact when there are no usable cuts', () => {
  assert.deepEqual(sponsorBlockAudioArgs(undefined, 10), []);
  assert.deepEqual(sponsorBlockAudioArgs([{ ...sponsor(1, 2), type: 'poi' }], 10), []);
});

test('stops the download when SponsorBlock removes the entire recording', () => {
  assert.throws(() => sponsorBlockAudioArgs([sponsor(0, 5), sponsor(4, 10)], 10), /No audio remains/);
});

test('FFmpeg WebAssembly cuts the marked samples and joins the remaining audio', async () => {
  const require = createRequire(import.meta.url);
  // The core expects a worker location even when the test supplies the WASM bytes.
  globalThis.self = { location: { href: import.meta.url } };
  try {
    const ffmpeg = await require('@ffmpeg/core')({ wasmBinary: await readFile(require.resolve('@ffmpeg/core/wasm')) });
    const bytesPerSecond = 16000;
    const before = Buffer.alloc(3 * bytesPerSecond, 0x10);
    const marked = Buffer.alloc(4 * bytesPerSecond, 0x20);
    const after = Buffer.alloc(3 * bytesPerSecond, 0x30);
    ffmpeg.FS.writeFile('input.pcm', Buffer.concat([before, marked, after]));

    const args = sponsorBlockAudioArgs([sponsor(5, 7), sponsor(3, 6)], 10);
    const format = ['-f', 's16le', '-ar', '8000', '-ac', '1'];
    assert.equal(ffmpeg.exec(...format, '-i', 'input.pcm', ...args, ...format, 'cut.pcm'), 0);
    assert.deepEqual(Buffer.from(ffmpeg.FS.readFile('cut.pcm')), Buffer.concat([before, after]));
  } finally {
    delete globalThis.self;
  }
});
