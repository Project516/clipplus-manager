export function safeName(name, extension = 'mp3') {
  let stem = name.replace(/\.[^.]*$/, '').replace(/[\x00-\x1f\x7f\\/:*?"<>|]/g, '_').replace(/[. ]+$/, '');
  while (new TextEncoder().encode(stem).length > 246 - extension.length) stem = Array.from(stem).slice(0, -1).join('');
  return `${stem || '_'}.${extension}`;
}

export function releaseSave(link) {
  if (link.href.startsWith('blob:')) URL.revokeObjectURL(link.href);
  link.removeAttribute('href');
  link.hidden = true;
}

export function offerSave(link, blob, name) {
  releaseSave(link);
  link.href = URL.createObjectURL(blob);
  link.download = name;
  link.hidden = false;
}

export function explain(error) {
  const message = String(error?.message ?? error);
  if (message.includes('Unable to communicate with SponsorBlock API')) {
    return 'SponsorBlock is unavailable. Clear “Remove sponsored segments with SponsorBlock” to download the original audio.';
  }
  if (message.includes('Failed to fetch')) {
    return 'Could not reach your proxy. Check the proxy URL in Settings, and that the proxy allows this site.';
  }
  return `Could not make the MP3: ${message}`;
}

export function validateMp3(bytes) {
  let offset = 0;
  if (bytes.length >= 10 && bytes[0] === 0x49 && bytes[1] === 0x44 && bytes[2] === 0x33) {
    if (bytes[3] !== 3 || bytes.slice(6, 10).some((byte) => byte > 127)) throw new Error('The MP3 tag is invalid.');
    offset = 10 + (bytes[6] << 21) + (bytes[7] << 14) + (bytes[8] << 7) + bytes[9];
    if (offset > 1024 * 1024) throw new Error('The MP3 tag is too large for the Sansa.');
  }
  if (offset + 4 > bytes.length || bytes[offset] !== 0xff || (bytes[offset + 1] & 0xfe) !== 0xfa) {
    throw new Error('The output is not a valid MP3.');
  }
}
