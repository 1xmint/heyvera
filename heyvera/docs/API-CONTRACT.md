# HeyVera FE ↔ BE Contract Matrix

Generated as Phase 0 lock. Frontend source: `heyvera/src/api/social.ts`, `pulse.ts`.  
Backend source: `crates/api/src/lib.rs` → `build_heyvera_router`.

Legend: **OK** mounted + used · **PAUSED** deliberately fail-closed · **MISSING** FE calls / needs route · **PARTIAL** works with caveats · **ORPHAN FE** helper unused or dead

## Profiles

| FE usage | Method + path | Backend | Notes |
|----------|---------------|---------|-------|
| My profile | `GET/POST/PATCH /v1/social/me/profile` | OK | Primary |
| My profile alt | `GET /v1/social/profile/me` | OK | Duplicate shape |
| Create profile | `POST /v1/social/profiles` | OK | |
| List / featured | `GET .../profiles`, `.../featured` | OK | Featured = first rows |
| User by handle | `GET /v1/social/users/{handle}` | OK | Bare profile object |
| Profile by handle | `GET /v1/social/profiles/{handle}` | OK | Alias: `{ profile, linkedAgents }` |
| Linked agents | `GET .../profiles/{handle}/linked-agents` | OK | Viewer-aware; public DTO never returns key material |
| My linked agents | `GET/POST /v1/social/linked-agents` | OK | Create returns `agentKey` once (`hvak_…`); list shows prefix |
| Rotate agent key | `POST /v1/social/linked-agents/{id}/rotate-key` | OK | Clerk only; new secret once; invalidates old hash |
| Followers / following lists | `GET .../followers`, `.../following` | OK | Viewer-aware; profile policy and blocks apply |
| PATCH profile (short path) | `PATCH /v1/social/profile` | OK | Alias of `/me/profile` |
| Follow status | `GET /v1/social/follows/{handle}/status` | OK | Returns `following` and `pending`; blocked/inactive/inaccessible profiles are concealed |
| Incoming follow requests | `GET /v1/social/follow-requests` | OK | Authenticated target only; requester profile policy is revalidated |
| Approve / reject request | `POST .../follow-requests/{id}/approve`, `DELETE .../follow-requests/{id}` | OK | Atomic resolution; approval creates the follow edge |
| Bookmarks list | `GET /v1/social/bookmarks` | OK | Auth; keyset cursor |

## Feed & posts

| FE usage | Method + path | Backend | Notes |
|----------|---------------|---------|-------|
| Home feed | `GET /v1/social/feed/home` | OK | Keyset cursor |
| Following feed | `GET /v1/social/feed/following` | OK | Authorization-aware keyset scan; cursor derives from the last visible row |
| Create post | `POST /v1/social/posts` | OK | Strict `public`/`followers`/`mutuals`/`guild`/`circle`/`author-only` type; Guild membership required; circle creation currently fails closed. Dual Clerk/agent auth |
| Get / delete post | `GET/DELETE /v1/social/posts/{id}` | OK | Viewer-aware centralized audience/profile/block/Guild policy; inaccessible and missing both 404 |
| Like / repost / bookmark | POST+DELETE on post actions | OK | Writes reauthorize target; quote/repost require an unprotected public source |
| User posts | `GET /v1/social/users/{handle}/posts` | OK | Viewer-aware and policy-filtered |
| Follow | `POST/DELETE /v1/social/follows/{handle}` | OK | Returns `following` or `pending`; retries are idempotent and do not duplicate request notifications |

## Explore

| FE usage | Method + path | Backend | Notes |
|----------|---------------|---------|-------|
| Search | `GET /v1/social/search` | PARTIAL | SQL LIKE; post results use authorization-safe keyset pagination, but profiles still have only an initial filled page |
| Trending | `GET /v1/social/trending` | PARTIAL | Hashtag counts |

## Communities

| FE usage | Method + path | Backend | Notes |
|----------|---------------|---------|-------|
| List | `GET /v1/social/communities` | OK | Public discover only |
| Create | `POST /v1/social/communities` | OK | Owner auto-joined as `owner` |
| Feed / join / leave | by `{id}` | OK | Every feed read requires membership; private non-member access is concealed as **404** — redeem invite |
| Mine | `GET .../communities/mine` | OK | Includes `role` |
| Members | `GET .../communities/{id}/members` | OK | Membership required for every Guild |
| Invites create/list | `POST/GET .../communities/{id}/invites` | OK | Owner only; token once on create |
| Invite revoke | `DELETE .../communities/{id}/invites/{inviteId}` | OK | Owner soft-revoke |
| Invite redeem | `POST /v1/social/invites/{token}/redeem` | OK | Joins member; `hvinv_` tokens |

## Messaging & notifications

| FE usage | Method + path | Backend | Notes |
|----------|---------------|---------|-------|
| Conversations | `GET/POST /v1/social/conversations`, `GET /conversations/{id}`, `GET /conversations/unread-count` | OK | Authorization is applied before a stable activity-keyset limit; randomized encrypted cursors are viewer-bound; a concealed detail read supports deep links and the aggregate unread count is uncapped; new threads enforce the canonical `everyone`/`verified`/`following`/`mutuals`/`nobody` consent policy while existing authorized threads remain reopenable; creation is transactional and retry-safe |
| Direct-message start | `POST /v1/social/direct-message-starts` | OK | Atomically sends the first message to an existing/policy-qualified direct thread or creates one durable request; client IDs are lifetime-idempotent, replays reauthorize current account/block access, target-specific denials are concealed, and all starts share a sliding 20/day ledger |
| Message requests | `GET /v1/social/message-requests`; `POST .../{id}/accept|decline|spam`; `DELETE .../{id}` | OK | Recipient-only inbox/spam pages use randomized encrypted `hvr1` cursors bound to viewer and bucket, expose an uncapped pending count plus relationship/shared-Guild context, and keep pending content outside conversation/unread/WebSocket paths; state transitions, accepted-message materialization, spam/block cleanup, dual 50-pending caps, and reciprocal supersession are transactional |
| Messages | `GET/POST .../conversations/{id}/messages` | OK | Backward history and forward recovery use separate encrypted, viewer/conversation-bound cursors; sends are bounded and idempotent by client ID; responses are private/no-store |
| DM read receipt | `POST .../conversations/{id}/read` | OK | Explicit monotonic per-participant watermark through a message ID; GET does not mutate read state |
| DM WebSocket ticket | `POST /v1/social/ws-ticket` | OK | Bearer auth; 30-second, hashed, one-use ticket; `Cache-Control: no-store` |
| DM WebSocket | `GET /v1/social/ws?ticket=...` | PARTIAL | Exact production `Origin`, atomically consumed ticket, bounded frames/subscriptions, read events, delivery-time policy revalidation, explicit slow-consumer gaps, and subscribe-then-HTTP catch-up; fan-out remains process-local |
| Notifications | `GET /v1/social/notifications` | OK | Actor and referenced-post access are revalidated; protected follow-request/acceptance types supported |
| Mark read | `POST /v1/social/notifications/read` | OK | Marks all |

## Media

| FE usage | Method + path | Backend | Notes |
|----------|---------------|---------|-------|
| Upload URL | `POST /v1/social/media/upload-url` | PARTIAL | Owner/size/type-bound row and short-lived PUT; local mock is disabled in production, but hostile-file quarantine/inspection is not built |
| Finalize | `POST /v1/social/media/{id}/finalize` | PARTIAL | Rechecks owner/status and verifies object existence before ready; byte signatures/scanning/derivatives remain |
| Authorized delivery | `GET /v1/social/media/{id}/content?...` | OK | Five-minute HMAC capability bound to media/post/viewer; current attachment and post policy are rechecked; storage key is never returned |

## Pulse

| FE usage | Method + path | Backend | Notes |
|----------|---------------|---------|-------|
| Drafts CRUD-ish | `/v1/pulse/drafts*` | OK | approve/reject/publish/audit |
| Create draft | `POST /v1/pulse/drafts` | PAUSED | Returns `503 SOCIALS_BILLING_UNAVAILABLE`; creates no draft and never reads or writes Cortex credits |
| Chat / tools agent | `POST /v1/pulse/chat` | PARTIAL | Read/transition tools remain; `create_draft` returns `SOCIALS_BILLING_UNAVAILABLE` and creates no draft |

## Agent bearer auth

Linked agents receive a server-generated `hvak_` API key on create/rotate. The plaintext secret is returned **once**; the DB stores SHA-256 hex in `agent_key_hash` and a display prefix in `agent_key`.

- Header: `Authorization: Bearer hvak_…` or `Authorization: Agent hvak_…`
- Scoped writes: `POST /v1/social/posts`, `POST /v1/pulse/drafts` only (other social writes stay Clerk-only)
- Lookup requires `link_state = 'active'`; suspended owner account is rejected when mappable

## Phase 0 / Phase 1 priorities

1. Preserve the centralized policy boundary while adding Circles, moderator roles, embeds, and caches.
2. Replace scan/hydration N+1 work with policy-aware SQL/batching and add independent profile-search cursors.
3. Build the hostile-media quarantine/inspection/derivative pipeline before public upload access.
4. Add seeded multi-principal E2E for protected follow approval, revocation, blocks, Guilds, media, and DMs.

Do not invent second path dialects. Prefer fixing FE to match `build_heyvera_router`.
