// Port of core/src/musicbrainz.rs. Keep the two in step.
import { formatDuration } from './soulseek.js';

const RECORDINGS = 'https://musicbrainz.org/ws/2/recording/';
const RELEASES = 'https://musicbrainz.org/ws/2/release/';

// Lowercase words made of letters, digits and apostrophes.
export function words(text) {
  // Alphabetic plus numbers is Rust's is_alphanumeric, so desktop and web split words alike.
  return (text.toLowerCase().replaceAll('\u2019', "'").match(/[\p{Alphabetic}\p{N}']+/gu) ?? [])
    .filter((word) => /[\p{Alphabetic}\p{N}]/u.test(word));
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

// Closest match to the query first, one row per song.
export function parseRecordings(json, query) {
  if (!Array.isArray(json?.recordings)) throw new Error('no recordings');
  const wanted = new Set(words(query));
  const ranked = json.recordings.map((recording, index) => {
    const artist = (recording['artist-credit'] ?? []).map((credit) => `${credit.name}${credit.joinphrase ?? ''}`).join('');
    const extra = [...new Set(words(`${artist} ${recording.title}`))].filter((word) => !wanted.has(word)).length;
    const releases = recording.releases ?? [];
    return { recording, artist, extra, score: recording.score ?? 0, count: releases.length, index };
  });
  ranked.sort((a, b) => a.extra - b.extra || b.score - a.score || b.count - a.count || a.index - b.index);
  const seen = new Set();
  const songs = [];
  for (const { recording, artist } of ranked) {
    const key = JSON.stringify([recording.title.toLowerCase(), artist.toLowerCase()]);
    if (seen.has(key)) continue;
    seen.add(key);
    let best;
    let bestRank;
    for (const release of recording.releases ?? []) {
      const rank = releaseRank(release);
      if (!bestRank || earlier(rank, bestRank) < 0) [best, bestRank] = [release, rank];
    }
    songs.push({ id: recording.id, title: recording.title, artist, release: best?.title, lengthMs: recording.length ?? undefined });
  }
  return songs;
}

// Lower is better: studio albums, EPs, other albums, then singles and the rest.
function groupKind(group) {
  const secondary = group['secondary-types'] ?? [];
  if (group['primary-type'] === 'Album' && secondary.every((type) => type === 'Soundtrack')) return 0;
  if (group['primary-type'] === 'EP') return 1;
  if (group['primary-type'] === 'Album') return 2;
  return 3;
}

const credit = (credits) => (credits ?? []).map((c) => `${c.name}${c.joinphrase ?? ''}`).join('');

// One row per release group. Albums with every query word come first, then studio albums.
export function parseReleases(json, query) {
  if (!Array.isArray(json?.releases)) throw new Error('no releases');
  const found = new Map();
  for (const release of json.releases) {
    const group = release['release-group'];
    if (!group?.id) continue;
    const year = release.date?.length >= 4 ? release.date.slice(0, 4) : undefined;
    const tracks = release['track-count'] ?? 0;
    const entry = found.get(group.id);
    if (entry) {
      if (tracks > 0 && (!entry.album.tracks || tracks < entry.album.tracks)) entry.album.tracks = tracks;
      if (year && (!entry.album.year || year < entry.album.year)) entry.album.year = year;
      entry.score = Math.max(entry.score, release.score ?? 0);
      entry.releases += 1;
      continue;
    }
    found.set(group.id, {
      album: { id: group.id, title: group.title ?? release.title, artist: credit(release['artist-credit']), year, tracks },
      kind: groupKind(group),
      score: release.score ?? 0,
      releases: 1,
      index: found.size,
    });
  }
  const wanted = new Set(words(query));
  const ranked = [...found.values()].map((entry) => {
    const have = new Set(words(`${entry.album.artist} ${entry.album.title}`));
    const missing = [...wanted].filter((word) => !have.has(word)).length;
    const extra = [...have].filter((word) => !wanted.has(word)).length;
    return { ...entry, missing, extra };
  });
  ranked.sort((a, b) => a.missing - b.missing || a.kind - b.kind || a.extra - b.extra
    || b.score - a.score || b.releases - a.releases || a.index - b.index);
  return ranked.map((entry) => entry.album);
}

export function releaseDetails({ artist, year, tracks }) {
  return [artist, year, tracks && `${tracks} ${tracks === 1 ? 'song' : 'songs'}`].filter(Boolean).join(' · ');
}

export function recordingDetails({ artist, release, lengthMs }) {
  return [artist, release, lengthMs && formatDuration(Math.floor(lengthMs / 1000))].filter(Boolean).join(' · ');
}

const busyError = () => Object.assign(new Error('MusicBrainz is busy, try again in a moment'), { busy: true });

function pause(ms, signal) {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(signal.reason);
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener('abort', () => {
      clearTimeout(timer);
      reject(signal.reason);
    }, { once: true });
  });
}

async function requestJSON(url, signal) {
  let response;
  try {
    response = await fetch(url, { signal });
  } catch (error) {
    if (signal?.aborted) throw error;
    throw new Error(`MusicBrainz search: ${error?.message ?? error}`);
  }
  // MusicBrainz answers 503 when it is rate limiting.
  if (response.status === 503) throw busyError();
  if (!response.ok) throw new Error(`MusicBrainz search: HTTP ${response.status}`);
  let json;
  try {
    json = await response.json();
  } catch (error) {
    if (signal?.aborted) throw error;
    throw new Error(`MusicBrainz response: ${error?.message ?? error}`);
  }
  // Overload can also come as a 200 with an error body.
  if (json?.error) throw busyError();
  return json;
}

// One request per call, plus one retry when MusicBrainz is busy. Browsers cannot set User-Agent, so the page origin identifies the client.
async function search(endpoint, parse, query, { signal, retryDelayMs = 1500 } = {}) {
  if (!words(query).length) return [];
  const url = new URL(endpoint);
  // dismax takes the words as plain text and tolerates extra or missing ones.
  url.searchParams.set('query', query.trim().split(/\s+/).join(' '));
  url.searchParams.set('dismax', 'true');
  url.searchParams.set('fmt', 'json');
  // Each song or album repeats once per release, so ask for more than we show.
  url.searchParams.set('limit', '100');
  let json;
  try {
    json = await requestJSON(url, signal);
  } catch (error) {
    if (!error.busy) throw error;
    await pause(retryDelayMs, signal);
    json = await requestJSON(url, signal);
  }
  try {
    return parse(json, query);
  } catch (error) {
    throw new Error(`MusicBrainz response: ${error?.message ?? error}`);
  }
}

export const searchRecordings = (query, options) => search(RECORDINGS, parseRecordings, query, options);
export const searchAlbums = (query, options) => search(RELEASES, parseReleases, query, options);
