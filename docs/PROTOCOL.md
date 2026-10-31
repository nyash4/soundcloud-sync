# Протокол SoundCloud Sync

Контракт между сервером (`server/`, Rust) и веб-клиентом (`web/`). Любое изменение — сначала здесь.

Все времена — миллисекунды. «Серверное время» — Unix epoch ms по часам сервера.
Все идентификаторы — строки. JSON-поля в `camelCase`.

## 1. Комнаты и ссылки

- `roomId` — 10 символов base62 (публичная часть ссылки).
- `token` — 24 символа base62 (≈143 бит), непредсказуемый. Сервер хранит только SHA-256 токена.
- Ссылка приглашения: `https://<host>/r/<roomId>#<token>`. Токен в фрагменте, чтобы не попадал в логи и Referer.
- Клиент хранит в `localStorage` по ключу `scs:room:<roomId>` объект `{ token, clientId, name }`.
  `clientId` — случайный UUID, генерируется клиентом один раз на комнату; по нему сервер узнаёт участника при переподключении.

## 2. HTTP API

Все ответы — JSON. Ошибки: HTTP 4xx/5xx и тело `{ "error": "<code>", "message": "<человекочитаемо, по-русски>" }`.

### `POST /api/rooms`
Тело: `{}` → `201 { "roomId": "...", "token": "..." }`. Ограничение частоты по IP.

### `POST /api/rooms/:roomId/import`
Заголовок `Authorization: Bearer <token>`.
Тело:
```json
{ "url": "https://soundcloud.com/...", "clientId": "...", "commandId": "...", "trackIds": [123, 456] }
```
`trackIds` — только в режиме без ключа API (см. §6), иначе отсутствует.

Ответ `200`:
```json
{
  "added": 12,
  "collection": { "title": "Soulhack", "kind": "album" },   // или null для одиночного трека
  "unavailable": [ { "title": "Track name или null", "reason": "not_embeddable" } ],
  "missing": 1,          // позиции коллекции, которые API вообще не вернул (удалены/приватны)
  "revision": 42
}
```
Ответ `200` с `"needsClientExpansion": true, "playlistUrl": "https://api.soundcloud.com/playlists/18"` и `added: 0` —
сервер просит клиента развернуть коллекцию через виджет и повторить запрос с `trackIds` (§6).

Коды ошибок: `bad_token`, `room_not_found`, `bad_url` (не SoundCloud/не разрешённый хост),
`not_found` (SoundCloud не знает ссылку), `unsupported` (профиль пользователя, лента и т.п.),
`too_large` (больше лимита импорта), `nothing_playable` (ни одного доступного трека — в очередь ничего не добавлено),
`rate_limited`, `upstream` (SoundCloud недоступен).

`reason` недоступного трека: `not_embeddable`, `blocked` (гео/правообладатель), `preview_only` (только 30 с фрагмент), `not_streamable`, `unknown`.

Импорт публикуется в очередь **одной операцией** (одна `revision`), в конец очереди, в исходном порядке коллекции.
Повторный запрос с тем же `commandId` не добавляет треки второй раз, а возвращает тот же результат.
После успешного импорта всем участникам рассылается `state` и `notice` (§4.3).

### `GET /api/health` → `{ "ok": true, "soundcloudApi": true|false }`
`soundcloudApi=false` — сервер работает без ключа (режим oEmbed, §6).

### Статика
Всё остальное — файлы `web/dist`; `/r/*` отдаёт `index.html` (SPA).

## 3. WebSocket `/ws`

Одно сообщение — один JSON-объект с полем `type`. Максимальный размер входящего сообщения — 16 KiB.
Сервер шлёт WS Ping-кадры каждые 5 с (браузер отвечает Pong сам, даже в фоновой вкладке).
Любой входящий кадр (включая Pong) продлевает жизнь соединения; 15 с тишины → сервер закрывает сокет.

### 3.1. Клиент → сервер

```jsonc
{ "type": "hello", "roomId": "...", "token": "...", "clientId": "...", "name": "Ника" }
{ "type": "ping", "t0": 1730000000000 }                 // часы клиента (Date.now()/performance-based)
{ "type": "cmd", "commandId": "uuid", "name": "...", "args": { ... }, "revision": 41 }
{ "type": "ready",  "playbackId": "p17" }               // трек загружен, стоит на startMs, на паузе, звук разрешён
{ "type": "ended",  "playbackId": "p17" }               // дослушал до конца
{ "type": "error",  "playbackId": "p17", "code": "unavailable" | "load_failed" }
{ "type": "join",   "playbackId": "p17" }               // поздний участник подключился к текущей позиции и уже играет/стоит на паузе
{ "type": "position", "playbackId": "p17", "ms": 83000, "playing": true }  // раз в 5 с и при паузе
{ "type": "audio", "blocked": true }                    // браузер требует обычное нажатие на страницу
```

`hello` должен быть первым сообщением. Имя: 1–32 символа после trim.

Команды (`cmd.name`):

| name | args | revision |
|---|---|---|
| `queue.remove` | `{ "entryId" }` | не нужна |
| `queue.move` | `{ "entryId", "afterId": "<entryId>" \| null }` (null — в начало очереди) | обязательна |
| `play` | `{}` | не нужна |
| `pause` | `{}` | не нужна |
| `seek` | `{ "playbackId": "<текущий playbackId>", "ms": 83000 }` | не нужна |
| `next` | `{ "fromEntryId": "<текущий entryId>" \| null }` | не нужна |
| `rename` | `{ "name" }` | не нужна |

`next.fromEntryId` защищает от двойного нажатия: если текущая позиция уже другая — команда no-op с `ok: true`.

### 3.2. Сервер → клиент

```jsonc
{ "type": "welcome", "selfId": "<participantId>", "state": RoomState, "serverTime": 1730000000000 }
{ "type": "pong", "t0": ..., "serverTime": ... }
{ "type": "ack", "commandId": "...", "ok": true, "revision": 42 }
{ "type": "ack", "commandId": "...", "ok": false, "error": "conflict" | "not_found" | "bad_args" | "rate_limited", "message": "...", "state": RoomState }
{ "type": "state", "state": RoomState }                 // после каждого изменения, всем
{ "type": "notice", "level": "info" | "warn", "text": "Ника добавила альбом «Soulhack» — 12 треков", "at": serverTime }
{ "type": "fatal", "error": "bad_token" | "room_not_found" | "bad_hello" | "session_replaced", "message": "..." }  // затем сокет закрывается
```

При конфликте (`queue.move` с устаревшей `revision`) ack содержит актуальный `state`.
Клиент сам повторяет `queue.move` один раз, если обе позиции ещё существуют.
Повторная команда с уже обработанным `commandId` возвращает сохранённый ack и ничего не делает.

Новая сессия с тем же `clientId` заменяет старую. Перед закрытием старому сокету приходит
`fatal: session_replaced`: клиент останавливает плеер и не переподключается автоматически,
чтобы две вкладки не перехватывали соединение друг у друга. Любой `fatal` прекращает переподключения.

### 3.3. RoomState

```jsonc
{
  "roomId": "...",
  "revision": 42,
  "serverTime": 1730000000000,            // момент формирования снимка
  "participants": [ Participant ],
  "current": Entry | null,
  "queue": [ Entry ],                     // только ещё не начавшиеся, по порядку
  "history": [ Entry ],                   // последние 30 сыгранных, новые в конце
  "playback": {
    "phase": "idle" | "preparing" | "playing" | "waiting_end",
    "paused": false,
    "playbackId": "p17" | null,
    "startMs": 0,                         // с какой позиции трека готовиться (0, либо позиция после перезапуска сервера)
    "startAt": 1730000001500 | null,      // серверное время общего старта/возобновления; null — старт не назначен
    "anchor": { "at": 1730000001500, "ms": 0 } | null,
                                          // «в серверное время at трек был на позиции ms» — для позднего входа:
                                          // pos = anchor.ms + (serverNow - anchor.at), если не paused; иначе anchor.ms
    "readyDeadline": serverTime | null,   // когда истечёт ожидание готовности
    "endDeadline": serverTime | null      // когда истечёт ожидание отстающих
  }
}
```

**Entry**
```jsonc
{
  "id": "e8f3k2",                 // уникален для позиции очереди, даже если трек повторяется
  "trackId": 293,
  "widgetUrl": "https://api.soundcloud.com/tracks/293",   // с ?secret_token=... для приватных ссылок
  "permalink": "https://soundcloud.com/forss/flickermood",
  "title": "Flickermood",
  "artist": "Forss",
  "durationMs": 213000 | null,
  "artwork": "https://i1.sndcdn.com/...-t500x500.jpg" | null,
  "collection": { "title": "Soulhack", "kind": "album" | "playlist" } | null,
  "addedBy": "Ника"
}
```

**Participant**
```jsonc
{
  "id": "u3k9",
  "name": "Ника",
  "online": true,
  "status": "listening" | "paused" | "loading" | "waiting" | "offline" | "out" | "blocked" | "idle",
  "outReason": "joined_late" | "ready_timeout" | "end_timeout" | "unavailable" | "load_failed" | "offline" | null
}
```
Статусы для интерфейса: listening — слушает; paused — пауза; loading — загрузка/подготовка;
waiting — дослушал, ждёт остальных (или готов и ждёт старта); offline — нет связи;
out — временно вне общего воспроизведения (см. `outReason`); blocked — браузер ждёт жест пользователя; idle — ничего не играет.

## 4. Воспроизведение (сервер — источник истины)

Параметры (env): `READY_TIMEOUT_MS=20000`, `END_TIMEOUT_MS=30000`, `HEARTBEAT_TIMEOUT_MS=15000`, `START_LEAD_MS=1500`.

### 4.1. Барьер
У текущего `playbackId` есть множество **участников барьера** — тех, кого ждут.
При подготовке нового трека в барьер попадают все участники онлайн.
Из барьера выбывают: по таймауту готовности (`ready_timeout`), по таймауту окончания (`end_timeout`),
по недоступности трека (`unavailable`) или сбою загрузки (`load_failed`), по потере связи дольше `HEARTBEAT_TIMEOUT_MS` (`offline`).
Вошедший во время `playing`/`waiting_end` не входит в барьер (`out`, `joined_late`), пока не пришлёт `join`.
Вошедший во время `preparing` входит в барьер сразу.
Сообщения со старым `playbackId` игнорируются.

### 4.2. Фазы
1. **idle** — ничего не играет. `play` при непустой очереди: первая позиция очереди → `current`, переход в preparing.
2. **preparing** — новый `playbackId`, `startAt = null`, запускается таймер готовности (только пока не paused).
   Клиенты грузят `current.widgetUrl`, перематывают на `startMs`, ставят на паузу, шлют `ready`.
   Когда все участники барьера `ready` и не paused → `startAt = now + START_LEAD_MS`, `anchor = {at: startAt, ms: startMs}`, фаза playing.
   По таймауту: не приславшие `ready` выбывают (`ready_timeout`); если готов хоть один — старт.
   Если `error` прислали все участники барьера (или после таймаута готовых нет, а ошибки есть) — notice «трек недоступен», переход к следующей позиции.
   Если после таймаута готовых нет и ошибок нет (все blocked/офлайн) — остаёмся в preparing и ждём, таймер не перезапускается до нового `ready`.
3. **playing** — каждый клиент в `startAt` (пересчёт через смещение часов) вызывает play.
   Первый `ended` от участника барьера → фаза waiting_end, `endDeadline = now + END_TIMEOUT_MS`.
4. **waiting_end** — закончившие молчат со статусом waiting. Когда `ended` прислали все оставшиеся участники барьера
   или истёк `endDeadline` (неуспевшие выбывают с `end_timeout`) → `current` уходит в history, следующая позиция → preparing.
   Очередь пуста → idle, `current = null`, воспроизведение остановлено.

### 4.3. Пауза / Play / Next
- `seek`: ручная перемотка для всей комнаты. Позиция — неотрицательное целое число миллисекунд,
  ограничивается длительностью трека, если она известна. Создаётся новый `playbackId` для того же `current`,
  `startMs` становится выбранной позицией, фаза — preparing с новым барьером готовности.
  Состояние паузы сохраняется, очередь и история не меняются. Команда для устаревшего `playbackId` — no-op.
  Старые `ready`/`ended` не влияют на новый запуск. Уже загруженный трек клиент использует повторно.
- `pause` в любой фазе кроме idle: `paused = true`, `startAt = null`.
  В playing/waiting_end сервер фиксирует `anchor = {at: now, ms: позиция по предыдущему anchor}`; таймер `endDeadline` замораживается (запоминается остаток).
  В preparing таймер готовности останавливается.
- `play`:
  - idle → старт очереди (см. выше);
  - preparing + paused → `paused = false`, если все готовы — назначить старт, иначе перезапустить таймер готовности;
  - playing/waiting_end + paused → `paused = false`, `startAt = now + START_LEAD_MS`, `anchor = {at: startAt, ms: anchor.ms}`;
    клиенты, которые ещё слушают, продолжают со **своей** позиции в `startAt` (никаких перемоток); закончившие продолжают ждать;
    `endDeadline` = now + сохранённый остаток.
- `next`: текущая позиция → history, следующая → preparing (без ожидания дослушивания). Пустая очередь → idle. `paused` сохраняется.
- Каждое изменение → `revision += 1` и рассылка `state`.
- `notice` рассылаются при: импорте, пропуске недоступного трека, выбывании участника по таймауту, конце очереди, `next` (кто переключил).

### 4.4. Перезапуск сервера
Очередь, current, history, revision, счётчик playbackId и последняя известная позиция current (из `position`, не чаще раза в 5 с)
хранятся в SQLite. После перезапуска: если был `current` — фаза preparing, `paused = true`, `startMs` = сохранённая позиция.
Музыка не запускается, пока кто-то не нажмёт Play.

## 5. Клиент: поведение плеера

- Один скрытый `<iframe>` SoundCloud Widget (`https://w.soundcloud.com/player/?url=...`) с `allow="autoplay"`,
  `auto_play=false`, `show_recommendations=false`. Пользователь не взаимодействует с iframe напрямую; управление — только через наш UI.
- События виджета (PLAY/PAUSE/FINISH) **никогда** не превращаются в команды серверу. Они лишь обновляют локальный статус и отчёты (`ready`, `ended`, `position`).
- Если виджет начал играть без нашей команды или сменил трек (FINISH → автопереход), клиент немедленно ставит паузу.
- Подготовка (`ready`): `widget.load(url, {auto_play:false})` → READY → «прогрев»: минимальная ненулевая громкость, `play()`, дождаться первого PLAY_PROGRESS,
  `pause()`, `seekTo(startMs)`, вернуть громкость → `ready`. Если PLAY не пришёл за 2.5 с в активной вкладке — `audio {blocked:true}`;
  следующее обычное нажатие на страницу автоматически повторяет прогрев внутри обработчика жеста.
- Смещение часов: `ping` каждые 2 с первые 10 с, потом каждые 5 с; `offset = serverTime - (t0 + t1)/2`, берётся по выборке с минимальным RTT из последних 10.
- Таймеры старта и пингов — в Web Worker (не троттлятся в фоновой вкладке).
- Поздний вход (`playing`/`waiting_end`, сам не в барьере): автоматически загрузить трек,
  однократно вызвать `seekTo(pos)` по актуальному anchor после загрузки, play (или пауза, если paused) → `join`.
  Отдельного подтверждения звука нет: при блокировке браузером достаточно любого обычного нажатия на страницу.
- Начальная громкость — 10 из 100; сохранённый выбор пользователя имеет приоритет. Выход виджета — квадрат значения / 100.
- Таймауты загрузки и getters не истекают в скрытой вкладке или сразу после разморозки.
  Отсутствие ответа `getCurrentSound` означает `load_failed`, а не `unavailable`.
  При сбое виджет пересоздаётся до трёх раз с задержками 1/2/3 с; при возвращении в зависшую вкладку восстановление повторяется.
  Только исчерпав попытки в активной вкладке клиент сообщает серверу об ошибке; причина сохраняется в статусе и серверных логах.
- Переподключение: экспоненциальная задержка 0.5→8 с; после `welcome` клиент сверяет `playbackId`:
  совпадает и он в барьере — продолжает как есть; иначе действует по новому снимку.
- Сон/долгая приостановка вкладки: при возвращении (`visibilitychange`, разрыв по времени между тиками > 10 с) — переподключиться.

## 6. SoundCloud

- Разрешённые хосты ссылок: `soundcloud.com`, `www.soundcloud.com`, `m.soundcloud.com`, `on.soundcloud.com` (короткие ссылки — сервер
  раскрывает редирект, принимая только редиректы на soundcloud.com), `api.soundcloud.com` (`/tracks/<id>`, `/playlists/<id>`).
  Любые другие URL — `bad_url`. Сервер не загружает произвольные адреса.
- **С ключом** (`SC_CLIENT_ID` + `SC_CLIENT_SECRET`): OAuth client_credentials, `GET /resolve`, для коллекций —
  `GET /playlists/:id/tracks?linked_partitioning=true&limit=200&access=playable,preview,blocked` по `next_href` до конца.
  `missing = track_count - получено`.
- **Без ключа** (режим разработки/личного использования): метаданные через oEmbed (`https://soundcloud.com/oembed`).
  Одиночный трек — сразу. Для коллекции сервер отвечает `needsClientExpansion` + `playlistUrl`; клиент загружает коллекцию во второй
  скрытый виджет, читает `getSounds()` (ID всех треков в порядке коллекции), повторяет импорт с `trackIds`.
  Сервер для каждого ID делает oEmbed `https://api.soundcloud.com/tracks/<id>` (параллельно, не более 6 одновременно) — так в очередь
  попадают только реальные треки SoundCloud. Название коллекции сервер берёт из oEmbed самой коллекции.
