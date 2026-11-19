// Port of core/src/musicbrainz.rs. Keep the two in step.
import { formatDuration } from './soulseek.js';

const ENDPOINT = 'https://musicbrainz.org/ws/2/recording/';

// Every word must be in the artist or the title, so "artist song" finds the song.
export function lucene(query) {
  return query
    .split(/\s+/)
    .filter((word) => /[\p{L}\p{N}]/u.test(word))
    .map((word) => {
      const escaped = Array.from(word, (c) => ('+-&|!(){}[]^"~*?:\\/'.includes(c) ? `\\${c}` : c)).join('');
      return `(artist:${escaped} OR recording:${escaped})`;
    })
    .join(' AND ');
}

// Lower is better: official studio albums first, then the earliest.
function releaseRank(release) {
  const official = release.status === 'Official';
  const group = release['release-group'];
  const album = group?.['primary-type'] === 'Album';
  // Film albums like "Help!" are tagged Soundtrack but hold the original songs.
  const studio = album && (group['secondary-types'] ?? []).every((type) => type === 'Soundtrack');
  let kind = 3;
  if (official && studio) kind = 0;
  else if (official && album) kind = 1;
  else if (official) kind = 2;
  return { kind, date: release.date || '9999' };
}

const earlier = (a, b) => a.kind - b.kind || (a.date < b.date ? -1 : a.date > b.date ? 1 : 0);

// One row per song: keep the copy on the best release.
export function parseRecordings(json) {
  if (!Array.isArray(json?.recordings)) throw new Error('no recordings');
  const songs = [];
  for (const recording of json.recordings) {
    const artist = (recording['artist-credit'] ?? []).map((credit) => `${credit.name}${credit.joinphrase ?? ''}`).join('');
    let best;
    let bestRank;
    for (const release of recording.releases ?? []) {
      const rank = releaseRank(release);
      if (!bestRank || earlier(rank, bestRank) < 0) [best, bestRank] = [release, rank];
    }
    const rank = bestRank ?? { kind: 4, date: '' };
    const song = {
      id: recording.id,
      title: recording.title,
      artist,
      release: best?.title,
      lengthMs: recording.length ?? undefined,
    };
    const kept = songs.find((other) => other.song.title.toLowerCase() === song.title.toLowerCase()
      && other.song.artist.toLowerCase() === song.artist.toLowerCase());
    if (!kept) songs.push({ song, rank });
    else if (earlier(rank, kept.rank) < 0) Object.assign(kept, { song, rank });
  }
  // Stable, so MusicBrainz's order holds within each kind.
  return songs.sort((a, b) => a.rank.kind - b.rank.kind).map(({ song }) => song);
}

export function recordingDetails({ artist, release, lengthMs }) {
  return [artist, release, lengthMs && formatDuration(Math.floor(lengthMs / 1000))].filter(Boolean).join(' · ');
}

// One request per call. Browsers cannot set User-Agent, so the page origin identifies the client.
export async function searchRecordings(query, { signal } = {}) {
  if (!lucene(query)) return [];
  const url = new URL(ENDPOINT);
  url.searchParams.set('query', lucene(query));
  url.searchParams.set('fmt', 'json');
  // Each song repeats once per release, so ask for more than we show.
  url.searchParams.set('limit', '100');
  let response;
  try {
    response = await fetch(url, { signal });
  } catch (error) {
    if (signal?.aborted) throw error;
    throw new Error(`MusicBrainz search: ${error?.message ?? error}`);
  }
  // MusicBrainz answers 503 when it is rate limiting.
  if (response.status === 503) throw new Error('MusicBrainz is busy, try again in a moment');
  if (!response.ok) throw new Error(`MusicBrainz search: HTTP ${response.status}`);
  try {
    return parseRecordings(await response.json());
  } catch (error) {
    if (signal?.aborted) throw error;
    throw new Error(`MusicBrainz response: ${error?.message ?? error}`);
  }
}
