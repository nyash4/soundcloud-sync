import { Widget } from './widget';

const POLL_MS = 800;
const MAX_POLLS = 25;

/**
 * Reads the ordered track IDs of a playlist through a second, silent widget (§6, no-API-key mode).
 * The widget fills its sound list lazily, so we poll until two reads agree and every item has an id.
 */
export async function expandPlaylist(url: string, onProgress: (count: number) => void): Promise<number[]> {
  const widget = new Widget();
  try {
    await widget.load(url);
    let previous = -1;
    for (let i = 0; i < MAX_POLLS; i++) {
      const sounds = (await widget.sounds()) ?? [];
      onProgress(sounds.length);
      const ids = sounds.map((s) => s.id);
      const complete = ids.length > 0 && ids.every((id): id is number => typeof id === 'number');
      if (complete && ids.length === previous) return ids as number[];
      previous = complete ? ids.length : -1;
      await new Promise((r) => setTimeout(r, POLL_MS));
    }
    throw new Error('unstable');
  } finally {
    widget.destroy();
  }
}
