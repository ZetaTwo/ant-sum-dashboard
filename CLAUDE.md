# ant-sum-dashboard

Only the frontend follows the fullstack-webapp skill; the backend is a
single-binary axum app with in-memory state and no database, auth or REST
API.

## Deviations from fullstack-webapp

- 2026-10-20 — No `vue-router` — a single view, nothing to route — Frontend
- 2026-10-20 — `api/ws.ts` (WebSocket with stall detection and reconnect) instead of the REST `api/client.ts` — the backend pushes state over `/ws`; there is no REST API or session to refresh — Frontend
- 2026-10-20 — No Playwright browser tests yet — the skill's setup is built around Postgres and auth, neither of which exists here; unit/component tests only for now — Frontend testing
- 2026-10-20 — Theme `dashboard` is dark only, without a light `prefers-color-scheme` variant — it runs on a dedicated screen in the training room — Frontend (Theming)
- 2026-10-20 — TypeScript pinned to `~6.0` — pnpm resolved TypeScript 7 (the native compiler) by default; the current `create-vite` vue-ts template pins `~6.0.3` for vue-tsc — Frontend (Stack)
