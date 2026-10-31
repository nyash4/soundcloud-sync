import type { Playback } from './protocol';

export function formatTime(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms)) return '–:––';
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

/** Russian plural: plural(5, ['трек', 'трека', 'треков']) → 'треков'. */
export function plural(n: number, forms: readonly [string, string, string]): string {
  const mod10 = n % 10;
  const mod100 = n % 100;
  if (mod10 === 1 && mod100 !== 11) return forms[0];
  if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return forms[1];
  return forms[2];
}

export const TRACKS = ['трек', 'трека', 'треков'] as const;

/** Room position by the anchor (§3.3): where the shared timeline is at server time `now`. */
export function anchorPosition(pb: Playback, now: number): number | null {
  if (!pb.anchor) return null;
  const elapsed = pb.paused ? 0 : Math.max(0, now - pb.anchor.at);
  return pb.anchor.ms + elapsed;
}
