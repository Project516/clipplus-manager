import coreURL from '../node_modules/@ffmpeg/core/dist/esm/ffmpeg-core.js?url';
import wasmURL from '../node_modules/@ffmpeg/core/dist/esm/ffmpeg-core.wasm?url';

export { FFmpeg } from '@ffmpeg/ffmpeg';
export const ffmpegCore = { coreURL, wasmURL };
