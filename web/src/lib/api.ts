import type { ApiErrorBody, CreatedRoom, ImportRequest, ImportResult, UnavailableItem } from './protocol';

export class ApiError extends Error {
  constructor(
    readonly code: string,
    message: string,
    /** `nothing_playable` lists why each track was rejected. */
    readonly unavailable: UnavailableItem[] = [],
  ) {
    super(message);
  }
}

const FALLBACK: Record<string, string> = {
  bad_token: 'Ключ комнаты не подходит.',
  room_not_found: 'Комната не найдена.',
  bad_url: 'Это не ссылка SoundCloud.',
  not_found: 'SoundCloud не знает такой ссылки.',
  unsupported: 'Такую ссылку добавить нельзя — нужна ссылка на трек, альбом или плейлист.',
  too_large: 'Коллекция слишком большая для одного импорта.',
  nothing_playable: 'В этой ссылке нет ни одного доступного трека.',
  rate_limited: 'Слишком много запросов, попробуйте через минуту.',
  upstream: 'SoundCloud сейчас не отвечает.',
  bad_request: 'Сервер не понял запрос.',
  internal: 'Внутренняя ошибка сервера.',
  network: 'Сервер недоступен.',
};

async function request<T>(path: string, body: unknown, token?: string): Promise<T> {
  let res: Response;
  try {
    res = await fetch(path, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: JSON.stringify(body),
    });
  } catch {
    throw new ApiError('network', FALLBACK.network!);
  }
  const data = (await res.json().catch(() => null)) as unknown;
  if (!res.ok) {
    const err = data as Partial<ApiErrorBody> | null;
    const code = typeof err?.error === 'string' ? err.error : `http_${res.status}`;
    const message = err?.message || FALLBACK[code] || `Ошибка сервера (${res.status}).`;
    throw new ApiError(code, message, Array.isArray(err?.unavailable) ? err.unavailable : []);
  }
  return data as T;
}

export function createRoom(): Promise<CreatedRoom> {
  return request<CreatedRoom>('/api/rooms', {});
}

export function importLink(roomId: string, token: string, body: ImportRequest): Promise<ImportResult> {
  return request<ImportResult>(`/api/rooms/${encodeURIComponent(roomId)}/import`, body, token);
}
