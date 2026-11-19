// Port of core/src/musicbrainz.rs. Keep the two in step.
import { formatDuration } from './soulseek.js';

const ENDPOINT = 'https://musicbrainz.org/ws/2/recording/';

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
export async function searchRecordings(query, { signal, retryDelayMs = 1500 } = {}) {
  if (!words(query).length) return [];
  const url = new URL(ENDPOINT);
  // dismax takes the words as plain text and tolerates extra or missing ones.
  url.searchParams.set('query', query.trim().split(/\s+/).join(' '));
  url.searchParams.set('dismax', 'true');
  url.searchParams.set('fmt', 'json');
  // Each song repeats once per release, so ask for more than we show.
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
    return parseRecordings(json, query);
  } catch (error) {
    throw new Error(`MusicBrainz response: ${error?.message ?? error}`);
  }
}
