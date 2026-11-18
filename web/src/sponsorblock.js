// Match yt-dlp's `--sponsorblock-remove default`, used by the desktop app.
const categories = ['sponsor', 'intro', 'outro', 'selfpromo', 'preview', 'interaction', 'music_offtopic', 'hook'];

export const sponsorBlockLookup = { key: 'SponsorBlock', when: 'after_filter', categories };

export function sponsorBlockAudioArgs(chapters, duration) {
  const end = Number.isFinite(duration) && duration > 0 ? duration : Infinity;
  const cuts = (chapters ?? [])
    .filter(({ category, type, start_time, end_time }) => type === 'skip' && categories.includes(category)
      && Number.isFinite(start_time) && Number.isFinite(end_time)
      && start_time >= 0 && start_time < end && end_time > start_time)
    .sort((a, b) => a.start_time - b.start_time);
  if (!cuts.length) return [];

  const keep = [];
  let start = 0;
  for (const cut of cuts) {
    if (cut.start_time > start) keep.push([start, cut.start_time]);
    start = Math.max(start, cut.end_time);
  }
  if (start < end) keep.push([start, Infinity]);
  if (!keep.length) {
    throw new Error('No audio remains after applying SponsorBlock. Clear the SponsorBlock checkbox to download the original audio.');
  }

  // Trim decoded samples to avoid the extra packets from stream-copy seeks.
  const trims = keep.map(([from, to]) =>
    `atrim=start=${from.toFixed(6)}${Number.isFinite(to) ? `:end=${to.toFixed(6)}` : ''}`);
  const timeline = 'asetpts=N/SR/TB';
  if (trims.length === 1) return ['-af', `${timeline},${trims[0]},${timeline}`];

  const inputs = trims.map((_, i) => `[input${i}]`);
  const outputs = trims.map((_, i) => `[keep${i}]`);
  return ['-af', [
    `${timeline},asplit=${trims.length}${inputs.join('')}`,
    ...trims.map((trim, i) => `${inputs[i]}${trim},${timeline}${outputs[i]}`),
    `${outputs.join('')}concat=n=${trims.length}:v=0:a=1`,
  ].join(';')];
}
