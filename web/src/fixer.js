// Port of plan() and verify() in core/src/rules.rs. Keep the two in step.
export const ID3_LIMIT = 1024 * 1024;

const LOSSLESS = ['alac', 'ape', 'wavpack', 'tta', 'mlp', 'truehd', 'flac'];
const MP3_TAGS = ['-id3v2_version', '3', '-write_id3v1', '0'];

export function id3Size(bytes) {
  if (bytes.length < 10 || bytes[0] !== 0x49 || bytes[1] !== 0x44 || bytes[2] !== 0x33) return 0;
  return 10 + ((bytes[6] << 21) | (bytes[7] << 14) | (bytes[8] << 7) | bytes[9]);
}

const positive = (value) => {
  const n = Number(value);
  return Number.isInteger(n) && n > 0 ? n : undefined;
};

export function parseProbe(json) {
  const root = JSON.parse(json);
  const info = { audio: undefined, hasArt: false, tags: {} };
  let audioStream;
  for (const stream of root.streams ?? []) {
    if (stream.codec_type === 'audio' && !info.audio) {
      info.audio = {
        codec: stream.codec_name ?? '',
        rate: positive(stream.sample_rate) ?? 0,
        bits: positive(stream.bits_per_raw_sample) ?? positive(stream.bits_per_sample),
        channels: positive(stream.channels) ?? 0,
      };
      audioStream = stream;
    } else if (stream.codec_type === 'video') {
      info.hasArt = true;
    }
  }
  for (const source of [root.format, audioStream]) {
    for (const [key, value] of Object.entries(source?.tags ?? {})) {
      if (typeof value === 'string') info.tags[key.toLowerCase()] ??= value;
    }
  }
  return info;
}

const isLossless = (codec) => codec.startsWith('pcm_') || LOSSLESS.includes(codec);

export function plan(info, tagBytes) {
  const { audio } = info;
  if (!audio) throw new Error('No audio stream found');
  const args = ['-map', '0:a:0', '-vn', '-map_metadata', '0'];
  const downmix = audio.channels > 2 ? ['-ac', '2'] : [];

  // mp3 plays at 44.1 and 48 kHz, so it is never re-encoded for sample rate.
  if (audio.codec === 'mp3' && !downmix.length) {
    return { ext: 'mp3', args: [...args, '-c:a', 'copy', ...MP3_TAGS] };
  }
  if (isLossless(audio.codec)) {
    const cdQuality = audio.rate === 44100 && audio.bits === 16;
    const copy = audio.codec === 'flac' && cdQuality && !downmix.length;
    return {
      ext: 'flac',
      args: [...args, ...(copy ? ['-c:a', 'copy'] : ['-c:a', 'flac', '-sample_fmt', 's16', '-ar', '44100', ...downmix])],
    };
  }
  return { ext: 'mp3', args: [...args, '-c:a', 'libmp3lame', '-q:a', '0', '-ar', '44100', ...downmix, ...MP3_TAGS] };
}

export function verify(info, tagBytes, ext) {
  const { audio } = info;
  if (!audio) return 'Output has no audio';
  if (audio.codec !== ext || audio.channels > 2) return `Output is ${audio.codec} with ${audio.channels} channels`;
  if (info.hasArt) return 'Cover art is still embedded';
  if (tagBytes > ID3_LIMIT) return 'The ID3 tag is still too large for the Sansa';
  if (ext === 'flac' && (audio.rate !== 44100 || audio.bits !== 16)) return `Output is ${audio.bits ?? '?'}-bit / ${audio.rate / 1000} kHz`;
  return undefined;
}

let probes = 0;

// ffprobe in @ffmpeg/core 0.12.10 returns -1 even on success, so a fresh output file is the signal.
async function probe(ffmpeg, file) {
  const output = `probe-${(probes += 1)}.json`;
  await ffmpeg.ffprobe(['-v', 'error', '-show_streams', '-show_format', '-of', 'json', file, '-o', output]);
  try {
    return parseProbe(new TextDecoder().decode(await ffmpeg.readFile(output)));
  } catch {
    throw new Error('The song could not be read.');
  }
}

// Tags to write, like fix_with_metadata: the given title and artist win, and a blank album falls back to the artist.
export function tagArgs(info, metadata) {
  const tags = metadata ? [['title', metadata.title], ['artist', metadata.artist]] : [];
  if (metadata?.album) tags.push(['album', metadata.album]);
  const artist = (metadata?.artist ?? info.tags.artist)?.trim();
  if (!metadata?.album && !info.tags.album?.trim() && artist) tags.push(['album', artist]);
  return tags.flatMap(([key, value]) => ['-metadata', `${key}=${value}`]);
}

// ffmpeg is an @ffmpeg/ffmpeg instance, or anything with the same four methods.
// metadata is optional: { title, artist, album }.
export async function fixForSansa(ffmpeg, bytes, extension, metadata) {
  const input = `input.${extension}`;
  await ffmpeg.writeFile(input, bytes);
  const info = await probe(ffmpeg, input);
  const { ext, args } = plan(info, id3Size(bytes));
  const output = `output.${ext}`;
  if (await ffmpeg.exec(['-i', input, ...args, ...tagArgs(info, metadata), '-f', ext, output]) !== 0) {
    throw new Error('The song could not be fixed for the Sansa.');
  }
  const fixed = await ffmpeg.readFile(output);
  const problem = verify(await probe(ffmpeg, output), id3Size(fixed), ext);
  if (problem) throw new Error(problem);
  return { bytes: fixed, ext };
}
