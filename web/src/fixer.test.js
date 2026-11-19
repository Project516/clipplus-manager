import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { test } from 'node:test';
import { fixForSansa, id3Size, parseProbe, plan, tagArgs, verify } from './fixer.js';

const audio = (codec, rate, bits, channels = 2) => ({ audio: { codec, rate, bits, channels }, hasArt: false, tags: {} });

test('plans the same fixes as the desktop rules', () => {
  assert.deepEqual(plan(audio('mp3', 48000), 0), {
    ext: 'mp3',
    args: ['-map', '0:a:0', '-vn', '-map_metadata', '0', '-c:a', 'copy', '-id3v2_version', '3', '-write_id3v1', '0'],
  });
  assert.deepEqual(plan(audio('flac', 44100, 16), 0).args.slice(-2), ['-c:a', 'copy']);
  assert.deepEqual(plan(audio('flac', 96000, 24), 0), {
    ext: 'flac',
    args: ['-map', '0:a:0', '-vn', '-map_metadata', '0', '-c:a', 'flac', '-sample_fmt', 's16', '-ar', '44100'],
  });
  assert.equal(plan(audio('alac', 44100, 16), 0).ext, 'flac');
  assert.equal(plan(audio('pcm_s16le', 44100, 16), 0).ext, 'flac');
  for (const codec of ['vorbis', 'opus', 'aac']) {
    assert.deepEqual(plan(audio(codec, 48000), 0).args.slice(5, 11), ['-c:a', 'libmp3lame', '-q:a', '0', '-ar', '44100']);
  }
  assert.deepEqual(plan(audio('mp3', 44100, undefined, 6), 0).args.slice(5, 13), ['-c:a', 'libmp3lame', '-q:a', '0', '-ar', '44100', '-ac', '2']);
  assert.throws(() => plan({ audio: undefined }, 0), /No audio/);
});

test('rejects output the Sansa will not play', () => {
  assert.equal(verify(audio('mp3', 44100), 100, 'mp3'), undefined);
  assert.equal(verify(audio('flac', 44100, 16), 0, 'flac'), undefined);
  assert.match(verify(audio('flac', 48000, 16), 0, 'flac'), /16-bit \/ 48 kHz/);
  assert.match(verify(audio('mp3', 44100, undefined, 6), 0, 'mp3'), /6 channels/);
  assert.match(verify({ ...audio('mp3', 44100), hasArt: true }, 0, 'mp3'), /Cover art/);
  assert.match(verify(audio('mp3', 44100), 2 * 1024 * 1024, 'mp3'), /ID3 tag/);
  assert.equal(verify({ audio: undefined }, 0, 'mp3'), 'Output has no audio');
});

test('reads ffprobe output like the desktop fixer', () => {
  const info = parseProbe(JSON.stringify({
    streams: [
      { codec_type: 'audio', codec_name: 'pcm_s16le', sample_rate: '44100', bits_per_raw_sample: 'N/A', bits_per_sample: 16, channels: 2, tags: { ALBUM: 'Stream' } },
      { codec_type: 'video', codec_name: 'mjpeg' },
    ],
    format: { tags: { Artist: 'Format', album: '' } },
  }));
  assert.deepEqual(info.audio, { codec: 'pcm_s16le', rate: 44100, bits: 16, channels: 2 });
  assert.equal(info.hasArt, true);
  assert.deepEqual(info.tags, { artist: 'Format', album: '' });
  assert.equal(id3Size(Uint8Array.from([0x49, 0x44, 0x33, 3, 0, 0, 0, 0, 1, 0])), 138);
  assert.equal(id3Size(Uint8Array.from([0xff, 0xfb])), 0);
});

test('writes the given tags and falls back to the artist as album', () => {
  const info = (tags) => ({ audio: undefined, hasArt: false, tags });
  const tag = (...pairs) => pairs.flatMap(([key, value]) => ['-metadata', `${key}=${value}`]);
  const song = { title: 'Song', artist: 'Band', album: 'Record' };
  assert.deepEqual(tagArgs(info({}), undefined), []);
  assert.deepEqual(tagArgs(info({ artist: ' Tagged ' }), undefined), tag(['album', 'Tagged']));
  assert.deepEqual(tagArgs(info({ artist: 'Tagged', album: 'Kept' }), undefined), []);
  assert.deepEqual(tagArgs(info({ artist: 'Tagged', album: 'Kept' }), song), tag(['title', 'Song'], ['artist', 'Band'], ['album', 'Record']));
  assert.deepEqual(tagArgs(info({ album: 'Kept' }), { title: 'Song', artist: 'Band' }), tag(['title', 'Song'], ['artist', 'Band']));
  assert.deepEqual(tagArgs(info({ artist: 'Tagged' }), { title: 'Song', artist: 'Band' }), tag(['title', 'Song'], ['artist', 'Band'], ['album', 'Band']));
  assert.deepEqual(tagArgs(info({ artist: 'Tagged' }), { title: 'Song', artist: ' ' }), tag(['title', 'Song'], ['artist', ' ']));
});

test('FFmpeg WebAssembly fixes cover art, hi-res FLAC, and surround audio', async (t) => {
  const require = createRequire(import.meta.url);
  // The core expects a worker location even when the test supplies the WASM bytes.
  globalThis.self = { location: { href: import.meta.url } };
  t.after(() => delete globalThis.self);
  const core = await require('@ffmpeg/core')({ wasmBinary: await readFile(require.resolve('@ffmpeg/core/wasm')) });
  const run = (command, args) => {
    core.setTimeout(-1);
    core[command](...args);
    const result = core.ret;
    core.reset();
    return result;
  };
  const ffmpeg = {
    writeFile: async (path, data) => core.FS.writeFile(path, data),
    readFile: async (path) => core.FS.readFile(path),
    exec: async (args) => run('exec', args),
    ffprobe: async (args) => run('ffprobe', args),
  };
  const probe = async (bytes, ext) => {
    await ffmpeg.writeFile(`check.${ext}`, bytes);
    await ffmpeg.ffprobe(['-v', 'error', '-show_streams', '-show_format', '-of', 'json', `check.${ext}`, '-o', `check-${ext}.json`]);
    return parseProbe(new TextDecoder().decode(await ffmpeg.readFile(`check-${ext}.json`)));
  };
  const fixture = (name) => readFile(new URL(`../test-fixtures/${name}`, import.meta.url));

  const art = await fixForSansa(ffmpeg, await fixture('art.mp3'), 'mp3');
  assert.equal(art.ext, 'mp3');
  assert.equal(art.bytes[3], 3, 'ID3v2.3 tag');
  const artInfo = await probe(art.bytes, 'mp3');
  assert.equal(artInfo.hasArt, false);
  assert.equal(artInfo.tags.title, 'Fixture Tone');
  assert.equal(artInfo.tags.album, 'Fixture Artist');

  const tagged = await fixForSansa(ffmpeg, await fixture('art.mp3'), 'mp3', { title: 'Given Title', artist: 'Given Artist', album: 'Given Album' });
  assert.deepEqual(
    (({ title, artist, album }) => ({ title, artist, album }))((await probe(tagged.bytes, 'mp3')).tags),
    { title: 'Given Title', artist: 'Given Artist', album: 'Given Album' },
  );
  const noAlbum = await fixForSansa(ffmpeg, await fixture('art.mp3'), 'mp3', { title: 'Given Title', artist: 'Given Artist' });
  assert.equal((await probe(noAlbum.bytes, 'mp3')).tags.album, 'Given Artist', 'album falls back to the given artist');

  const hires = await fixForSansa(ffmpeg, await fixture('hires.flac'), 'flac');
  assert.equal(hires.ext, 'flac');
  assert.deepEqual((await probe(hires.bytes, 'flac')).audio, { codec: 'flac', rate: 44100, bits: 16, channels: 2 });

  const surround = await fixForSansa(ffmpeg, await fixture('surround.ogg'), 'ogg');
  assert.equal(surround.ext, 'mp3');
  const surroundAudio = (await probe(surround.bytes, 'mp3')).audio;
  assert.equal(surroundAudio.channels, 2);
  assert.equal(surroundAudio.rate, 44100);
});
