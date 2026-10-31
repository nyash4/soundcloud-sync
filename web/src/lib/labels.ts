import type { OutReason, ParticipantStatus, UnavailableReason } from './protocol';

// Wording avoids gendered verb forms: statuses are nouns or neutral phrases.

export const STATUS_LABEL: Record<ParticipantStatus, string> = {
  listening: 'слушает',
  paused: 'пауза',
  loading: 'загрузка',
  waiting: 'ждёт остальных',
  offline: 'нет связи',
  out: 'вне воспроизведения',
  blocked: 'нужно включить звук',
  idle: 'в комнате',
};

export const OUT_REASON: Record<OutReason, string> = {
  joined_late: 'вход посреди трека',
  ready_timeout: 'долгая загрузка',
  end_timeout: 'не дослушано вовремя',
  unavailable: 'трек недоступен',
  load_failed: 'сбой загрузки',
  offline: 'нет связи',
};

export const UNAVAILABLE_REASON: Record<UnavailableReason, string> = {
  not_embeddable: 'автор запретил встраивание',
  blocked: 'заблокирован в регионе или правообладателем',
  preview_only: 'доступен только 30-секундный фрагмент',
  not_streamable: 'нельзя воспроизвести',
  unknown: 'причина неизвестна',
};
