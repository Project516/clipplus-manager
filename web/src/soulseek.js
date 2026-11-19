// Formats Rockbox plays on the Clip+.
export const PLAYABLE = ['mp3', 'flac', 'ogg', 'opus', 'm4a'];
export const RESULT_LIMIT = 200;

export function secureRelayURL(value) {
  const url = new URL(value);
  if (url.protocol !== 'wss:' || url.username || url.password || url.search || url.hash) {
    throw new Error('Enter a secure WebSocket relay URL. Put the relay token in its own field.');
  }
  return url;
}

function rank(a, b) {
  return (b.extension === 'mp3') - (a.extension === 'mp3')
    || b.freeSlot - a.freeSlot
    || b.uploadSpeed - a.uploadSpeed;
}

export const fileKey = (file) => `${file.username}\n${file.path}`;

export function addResult(rows, { username, freeSlot, queueLength, uploadSpeed, files }, limit = RESULT_LIMIT) {
  const seen = new Set(rows.map(fileKey));
  const added = files
    .filter((file) => PLAYABLE.includes(file.extension))
    .map((file) => ({ ...file, username, freeSlot, queueLength, uploadSpeed }))
    .filter((file) => !seen.has(fileKey(file)) && seen.add(fileKey(file)));
  return [...rows, ...added].sort(rank).slice(0, limit);
}

// Port of is_audio in core/src/slskd.rs. Album folders count every audio file.
const AUDIO = ['mp3', 'flac', 'm4a', 'ogg', 'opus', 'wav', 'aac', 'wma', 'aiff', 'alac'];

const lastSeparator = (path) => Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
export const baseName = (path) => path.slice(lastSeparator(path) + 1);
export const albumKey = (username, folder) => `album\n${username}\n${folder}`;

export function parentFolder(path) {
  const end = lastSeparator(path);
  return end > 0 ? path.slice(0, end) : undefined;
}

function extensionOf(path) {
  const name = baseName(path);
  return name.includes('.') ? name.slice(name.lastIndexOf('.') + 1).toLowerCase() : '';
}

// Port of album_candidates: one group per user and folder. Adds to `groups` in place.
export function groupAlbumFiles(groups, { username, files }, limit = RESULT_LIMIT) {
  for (const { path } of files) {
    const extension = extensionOf(path);
    const folder = parentFolder(path);
    if (!AUDIO.includes(extension) || !folder) continue;
    const key = albumKey(username, folder);
    let group = groups.get(key);
    if (!group && groups.size >= limit) continue;
    if (!group) {
      group = { username, folder, name: baseName(folder), songs: 0, format: extension, counts: new Map(), paths: new Set() };
      groups.set(key, group);
    }
    if (group.paths.has(path)) continue;
    group.paths.add(path);
    group.songs += 1;
    const count = (group.counts.get(extension) ?? 0) + 1;
    group.counts.set(extension, count);
    if (count > group.counts.get(group.format)) group.format = extension;
  }
  return groups;
}

export const plural = (count, word) => `${count} ${word}${count === 1 ? '' : 's'}`;

export const albumDetails = ({ username, songs, format }) => [username, plural(songs, 'song'), format].join(' · ');

export function formatSize(bytes) {
  return `${(bytes / 1e6).toFixed(1)} MB`;
}

export function formatDuration(seconds) {
  const total = Math.round(seconds);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

export function fileDetails(file) {
  return [
    file.username,
    formatSize(file.size),
    file.bitrate && `${file.bitrate} kbps`,
    file.duration && formatDuration(file.duration),
  ].filter(Boolean).join(' · ');
}

// fraction: null shows a busy meter, 0 to 1 fills it.
export function describeProgress(progress) {
  switch (progress.state) {
    case 'queued':
      return { text: progress.position ? `Queued, position ${progress.position}` : 'Queued by the user', fraction: null };
    case 'transferring': {
      if (!progress.total) return { text: 'Downloading', fraction: null };
      const fraction = Math.min(1, progress.received / progress.total);
      return { text: `${Math.floor(fraction * 100)}% of ${formatSize(progress.total)}`, fraction };
    }
    default:
      return { text: 'Connecting to the user', fraction: null };
  }
}
