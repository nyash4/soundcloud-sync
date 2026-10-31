export type Route = { name: 'landing' } | { name: 'room'; roomId: string } | { name: 'not_found' };

function parse(): Route {
  const path = location.pathname;
  if (path === '/' || path === '') return { name: 'landing' };
  const m = /^\/r\/([A-Za-z0-9]+)\/?$/.exec(path);
  return m ? { name: 'room', roomId: m[1]! } : { name: 'not_found' };
}

export const router = $state<{ route: Route }>({ route: parse() });

addEventListener('popstate', () => {
  router.route = parse();
});

export function navigate(url: string): void {
  history.pushState(null, '', url);
  router.route = parse();
}

/** The room token travels in the fragment so it never reaches server logs (§1). */
export function hashToken(): string | null {
  const t = location.hash.slice(1);
  return /^[A-Za-z0-9]+$/.test(t) ? t : null;
}

export function inviteUrl(roomId: string, token: string): string {
  return `${location.origin}/r/${roomId}#${token}`;
}
