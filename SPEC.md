# Sunrise Discord bot: spec

Date: 2026-11-10. Source studied: Kira, `horizon-rust/kira` @ `21f4a0b` (shallow clone of `main`, read-only, at
`/projects/sunrise-bot/kira-src`). Kira is MIT, "Copyright (c) 2026 Ludovic Marie and the Kira contributors"; the
licence is repo-wide (`LICENSE`), its source files carry no per-file headers.

Working name for the new bot: **sunrise-bot** (call it whatever the team likes; nothing below depends on the name).

---

## 0. TL;DR

- Kira is ~35k lines of Node 22 / discord.js 14: 9.7k in `src/*.js`, 8k in `src/features/*.js`, 11.3k of tests,
  plus a 967-line README that documents every feature. About 80 % of it is Horizon-specific (governance, elections,
  applications, staff dashboards, sponsors, SMP market, YouTube videos, X, the hub/roadmap/journal, MCP for agents,
  AI moderation tuned to a 30-channel server with eleven language channels).
- Sunrise v1 keeps: **declarative server bootstrap**, **role buttons**, **GitHub webhook → #github-feed**, **site
  revalidate hook**, **moderation basics** (Discord AutoMod + a spam/flood filter + a few mod commands + #mod-log),
  **#support forum template check and `/done`**, **static FAQ**, and the **profiler-link helper**.
- Language: **Rust** (serenity + poise, axum, tokio, SQLite). Kira's code is ported, not pasted: what is reused
  verbatim is data (regex lists, templates, texts, test vectors); logic is translated module by module with its tests.
- Hosting: one Deployment in the `sunrise` namespace of the umceko cluster, next to `site`, with a small PVC for
  SQLite, a Secret for tokens, and an HTTPRoute `bot.playsunrise.net` for GitHub's webhook.

---

## 1. Inventory of Kira

### 1.1 Runtime shape

| Thing | Where | Notes |
|---|---|---|
| Entry | `src/main.js` (9) → `src/bot.js` (231) | `bot.js` wires 29 feature modules (`FEATURES`), merges their `commands`/`handlers`, runs the release poller, one-off posts, and the HTTP server |
| Discord client, shared state | `src/core.js` (58) | discord.js `Client` with Guilds, GuildMembers, GuildMessages, MessageContent, GuildModeration, GuildVoiceStates, GuildMessageReactions, GuildInvites, DirectMessages intents; `channel(key)`, `isStaff`, `modlog()`, `parseDuration` |
| State | `src/store.js` (48) | One JSON file (`KIRA_STATE`, default `data/state.json`), atomic write via `.tmp` + rename; setup and bot own disjoint top-level keys (`SETUP_KEYS`) so both may run at once |
| Config | `src/config.js` (991) | Every channel, role, text, rule, FAQ, threshold and Horizon ID, plus ~25 env flags. The single largest Horizon-specific file |
| HTTP server | `src/mcp.js` (286) | `node:http` + MCP SDK (Streamable HTTP); hosts every route in `bot.js` (`/health`, webhook, proxies, staff API, admin) |
| Server setup | `src/setup.js` (491), `src/layout-diff.js` (152), `src/permissions.js` (57), `src/adopt.js` (34), `src/screening.js` (32), `src/automod.js` (95), `src/archive.js` (77) | `npm run setup`: additive by default; `--apply-layout` prints a diff, `--apply-layout --yes` applies it |
| Secrets | `src/secrets.js` (49), `scripts/seal.mjs` | Railway-specific "sealed" env vars (RSA key on the volume) |
| Admin | `src/admin.js` (139), `scripts/admin.mjs` | `/admin/backup`, `/admin/restore`, contributor clean-up; off unless `KIRA_ADMIN_TOKEN_SHA256` |
| Deploy | `Dockerfile`, `docker-entrypoint.sh`, `railway.json`, `deploy/kira.service` | Railway (volume `/data`), fallback systemd user unit on Ludovic's ThinkPad |
| CI | `.github/workflows/test.yml` | `npm ci`, `node --check` on every file, `node --test` |
| Deps | `package.json` | `discord.js` ^14.22, `@anthropic-ai/sdk`, `@modelcontextprotocol/sdk`, `zod` |

Pattern worth keeping: every feature is split into a **kit** with no Discord and no network (`*-kit.js`, `board.js`,
`releases.js`, `layout-diff.js`, `permissions.js`) and a **feature** file that does the talking. Tests target the kits.

### 1.2 Features

Sizes are lines of the implementing files (tests not counted). "Ext" = external dependency beyond Discord.

| # | Feature | Files (lines) | Commands / jobs / routes | Ext | Config / env | Horizon-specific |
|---|---|---|---|---|---|---|
| 1 | **Server bootstrap** | `setup.js` 491, `layout-diff.js` 152, `permissions.js` 57, `adopt.js` 34, `screening.js` 32, `archive.js` 77, `texts.js` 178 (shared), `config.js` `LAYOUT`/`ROLES`/`RULES`/`WELCOME`/`FAQ`/`HOWTO`/`PINS` | `npm run setup [--apply-layout [--yes]]`; creates roles, categories, channels, forum tags, overwrites, Community mode, Onboarding, rules screening, AutoMod, permanent invite; keeps its own messages (welcome, roles, rules, FAQ, how-to posts) up to date; exports then deletes `REPLACED` channels | — | `DISCORD_TOKEN`, `GUILD_ID`, `KIRA_OWNER_ID`, `KIRA_STATE` | Whole `LAYOUT` (PLAYHORIZON.DEV, HORIZON-GENERAL, HORIZONSMP, languages…), Ludovic as owner, retired roles/channels from the old server |
| 2 | **Brand** | `brand.js` 29, `assets/*` | On start: re-uploads server icon and bot avatar when their file hash changes | — | — | Horizon icon, Kira avatar, banner |
| 3 | **Moderation filters** | `features/moderation.js` 597, `spam-kit.js` 60, `honeypot-kit.js` 52, `automod.js` 95, `config.js` `MOD` | Flood (8 msgs/10 s), repeat (3/min), cross-post, link flood, caps, emoji walls, >5 links, dangerous file extensions, links from accounts <3 days; timeout ladder 10 min → 1 h → 1 d; raid mode (15 young joins in 2 min); honeypot channel soft-ban; Discord AutoMod rules (spam, slurs, mention spam, invites/scams, piracy, Telegram) | — | `KIRA_HONEYPOT` | Piracy patterns name TLauncher etc. (generic to Minecraft); the honeypot channel name; staff = Horizon Team |
| 4 | **Mod commands** | `features/moderation.js` | `/warn` `/warnings` `/clearwarnings` `/timeout` `/untimeout` `/kick` `/ban` `/unban` `/purge` `/slowmode` `/lock` `/unlock` `/raidmode` `/report-member`, context menu "Report to mods" | — | — | Kick/ban limited to Horizon Team |
| 5 | **Mod cases, quarantine, appeals** | `features/modcases.js` 723, `modcase-kit.js` 452 | Numbered cases from the audit log in #mod-actions, `/case`, `/quarantine`, `/unquarantine`, `/appeal`, appeal threads with Uphold/Shorten/Lift buttons; web appeal for banned users | Website `/hub/appeal` via staff proxy, Discord OAuth | `DISCORD_OAUTH_CLIENT_SECRET`, `STAFF_SITE_URL` | Built to Horizon's mods' spec; Minecraft ops (`MC_OPS`) |
| 6 | **Batch + image AI moderation** | `features/batchmod.js` 568, `batch-kit.js` 405, `image-kit.js` 122, `ai.js` (part) | Windows of messages → one Haiku call; flags in #mod-flags with Confirm/Dismiss; CSAM image auto-quarantine; `/kira-batch` | Anthropic API | `KIRA_BATCH*` (6 vars), `ANTHROPIC_API_KEY` | Tuned on Horizon's traffic |
| 7 | **AI answers + AI moderation in every language** | `features/ai.js` 459, `ai.js` 355, `ai-kit.js` 341 | Mention/reply → one Claude answer from FAQ + server guide + horizon README/docs/release notes; hand-offs to the team; `/kira-ai`; `GET /ai-usage`; AI bug-report dedupe | Anthropic API, GitHub (docs) | `KIRA_AI`, `ANTHROPIC_API_KEY`, `KIRA_AI_DAILY_USD`, `KIRA_AI_BUGS`, `KIRA_AI_MOD`, `KIRA_USAGE_TOKEN`, `KIRA_AI_SOURCES`, `KIRA_HANDOFF_REMINDERS` | Sources are horizon-rust/horizon; link allowlist playhorizon.dev |
| 8 | **Logs** | `core.js` `modlog`, `features/moderation.js` | #mod-log (deletes, edits, nicknames, AutoMod), #join-leave (with invite tracking), #mod-actions | — | — | — |
| 9 | **Mod-mail** | `features/community.js` 223 (part), `features/modcases.js` (status) | DM the bot → thread in #mod-mail; `//` notes, `//close`; daily list of open threads | — | — | — |
| 10 | **Community** | `features/community.js`, `texts.js` | Welcome in #new-members (batched), role buttons, 👍/👎 on ideas, 🔥 in showcase, ⭐ hall of fame, `/remind`, `/help` `/links` `/horizon` `/userinfo` `/serverinfo` `/avatar` `/modmail` | — | `LINKS`, `HORIZON`, `STARBOARD`, `WELCOME` | Texts, links |
| 11 | **Chat clears** | `features/chatclear.js` 96, `clear-kit.js` 44 | Every 6 h delete messages older than 24 h in two chat channels; `/clear-chat` | — | `CLEAR` | Ludovic's policy |
| 12 | **Levels** | `features/levels.js` 104 | XP per minute of talk / voice; level roles; `/rank` `/leaderboard` `/givexp` | — | `LEVELS` | — |
| 13 | **Voice rooms** | `features/rooms.js` 131 | "➕ create your room" → personal channel; `/room …`, `/pull` | — | — | Ludovic's locked channel |
| 14 | **Forums** | `features/forums.js` 142, `texts.js` `howToPost`/`missingMessage` | Template check on new posts (labels + regex per forum), close after 1 h if still missing; one pinned "How to post" post per forum; `/done` sets the done tag and closes | — | `LAYOUT[].check`, `HOWTO`, `MOD.formatGraceMs` | Templates |
| 15 | **Support forum** | `features/support.js` 55, `support-kit.js` 19 | First AI answer, note to 🤝 Support (ping ≤ 1 per 15 min), only poster + Support may reply | (AI) | `SUPPORT` | Support role |
| 16 | **GitHub issues from forums** | `features/github.js` 425, `issuekit.js` 243 | #horizon-bugs posts and `/report` → issues; follow-ups → comments; GitHub answers → back into the post; `/issue` (staff) | GitHub (token or App) | `GITHUB_TOKEN`, `KIRA_GITHUB_DRY_RUN`, `GITHUB` | horizon-rust/horizon, ludovic111 as maintainer |
| 17 | **Releases feed** | `bot.js` `checkReleases`, `releases.js` 192, `feeds.js` 14 | Poll REST every 3 min (30 min while webhook healthy) → #releases, one daily ping of 📦 Releases | GitHub | `RELEASE_REPOS`, `RELEASE_EVERY_MS` | Tag scheme `nightly`, `launcher@…` |
| 18 | **GitHub webhook** | `webhook.js` 95, `notify-kit.js` `signatureOk` | `POST /github/webhook`: HMAC-SHA256 check, dedupe by `X-GitHub-Delivery`, owner filter, 202 then fan-out to listeners; `throttle()`, `healthy()` | GitHub | `KIRA_GITHUB_WEBHOOK_SECRET` | Owner `horizon-rust` |
| 19 | **Site revalidate + GitHub proxy** | `features/site.js` 130 | On webhook: `POST {site}/api/revalidate` with cache tags, ≤ 1 call/30 s per tag, then warms pages; `GET /github/proxy/repos/horizon-rust/{horizon,horizon-mods}/…` for the site | Website, GitHub App | `KIRA_SITE_REVALIDATE_SECRET`, `KIRA_SITE_URL`, `KIRA_SITE_DRY_RUN` | Tags `github:horizon:*`, `github:horizon-mods:main` |
| 20 | **Launcher GitHub fallback** | `launcher-github.js` 84 | Public `GET /github/public/…` cache (60 s) for launchers rate-limited by GitHub | GitHub App | — | horizon launcher |
| 21 | **GitHub clients** | `github.js` 189, `github-app.js` 56 | REST client with rate-budget floor, ETag cache persisted to disk; GitHub App JWT → installation token | GitHub App | `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY`, `GITHUB_TOKEN`, `KIRA_GITHUB_KEEP`, `KIRA_GITHUB_PUBLIC_KEEP`, `KIRA_GITHUB_ETAGS` | App id, org |
| 22 | **Work board + claims** | `features/workboard.js` 491, `board.js` 306, `workkit.js` 149 | Board built from GitHub every 10 min; `/claim` `/unclaim` `/working` `/board` `/move` `/link-github` `/mytasks` `/goodfirst`; `GET /contributors.json` | GitHub | `WORK` | Hub kanban |
| 23 | **Contributors + ranks** | `features/contributors.js` 447, `contributor-kit.js` 265 | `/verify-github` (code in bio or gist), Contributor role on first merged PR, ore ranks by merged PR count, `/ranks` | GitHub search | `CONTRIBUTORS` | Thresholds, protected reviewers |
| 24 | **Notifications** | `features/notify.js` 234, `notify-kit.js` 203 | `/notify dm:on` → batched DMs about your own PRs/issues (reviews, comments, merges, failing checks, conflicts) | GitHub webhook + GraphQL | `KIRA_NOTIFY_DRY_RUN`, `NOTIFY` | Repo list, check names |
| 25 | **Triage via bot** | `features/triage.js` 184, `triage-kit.js` 49 | `/gh label/close/reopen/duplicate/pause/resume` for trusted reviewers with the bot's token | GitHub | `TRIAGE` | Reviewer list, ludovic111 rules |
| 26 | **Hub front desk** | `features/hub.js` 212, `hub.js` 235 | `/report` (bug form), `/roadmap`, `/status`, `@Kira <verb>`; ideas → suggestion queue | Website `roadmap.json` | `HUB_URL`, `HUB_ROADMAP_URL`, `ROADMAP_SUGGESTIONS_CHANNEL_ID` | Hub, roadmap |
| 27 | **#updates feed** | `features/updates.js` 110, `journal.js` 74 | Reads `playhorizon.dev/data/journal.json` every 10 min; pinned roadmap | Website | `HUB_JOURNAL_URL` | Journal format |
| 28 | **X poster** | `features/x.js` 52, `xposter.js` 91 | Releases / weekly / shipped → X | X API | `X_*` | Horizon's X account |
| 29 | **YouTube videos** | `videos.js` 94 | Atom feed every 10 min → #ludovic-videos | YouTube | `VIDEOS` | Ludovic's channel |
| 30 | **One-off posts** | `bot.js` `postOneOffs`, `config.js` `ONE_OFFS` | Hand-written announcements posted once, optionally edited in place and pinned | — | — | Texts |
| 31 | **Agents (MCP)** | `mcp.js` 286, `agents.js` 195, `features/agents.js` 245 | MCP server (board tools), `/agent connect/tokens/revoke`, #agent-log | MCP SDK | `KIRA_MCP_URL`, `KIRA_AGENT_ISSUANCE`, `KIRA_AGENTS` | Board, `ludovic_now` |
| 32 | **Game chat** | `features/game.js` 92, `game-kit.js` 78 | `POST /game/ask` for a Minecraft server plugin (kira-pumpkin) | Pumpkin plugin, Anthropic | `KIRA_GAME_SERVERS` | Pumpkin |
| 33 | **SMP market** | `features/smp.js` 98, `smp-kit.js` 75 | `POST /smp/ingest/*.json`, `GET /smp/*.json`, daily summary to #server-chat | SMP mod | `KIRA_SMP_INGEST_TOKEN` | Horizon SMP |
| 34 | **Sponsors** | `features/sponsors.js` 149, `sponsor-kit.js` 62 | `/sponsor`, `/sponsor-grant`, daily recheck via GitHub `isSponsoredBy` | GitHub GraphQL (Ludovic's token) | `GITHUB_TOKEN`, `SPONSORS` | Ludovic's sponsors |
| 35 | **Staff dashboards + applications** | `staff-*.js` (auth 151, http 180, store 47, hooks 71, counters 56), `features/staff*.js` (155+228+114+63), `applications.js` 103, `application-votes.js` 265, `apply-kit.js` 27, `features/apply.js` 104, `team-data.js` 161, `team-stats.js` 134, `docs/staff-api.md` | `/staff password`, `/apply`, `/staff/api/*` behind the website's proxy, votes, veto window | Website, GitHub GraphQL | `STAFF_PROXY_KEY`, `STAFF_SITE_URL`, `KIRA_STAFF`, `KIRA_APPLICATIONS`, `APPLY_FORM_LIVE` | Ludovic's veto, playhorizon.dev/hub/team |
| 36 | **Team roles / team votes** | `features/teamroles.js` 70, `team-roles-kit.js` 52, `features/teamvote.js` 119, `team-vote-kit.js` 56 | `/team add/remove/list`, `/team-vote propose/remove` (48 h, 2/3) | — | `TEAMS`, `TEAM_VOTE` | Horizon Team |
| 37 | **Governance: polls, elections, 18+** | `features/poll.js` 357, `poll-kit.js` 155, `features/elections.js` 728, `elections-kit.js` 304, `election-evidence.js` 56, `governance-kit.js` 57, `decisions-kit.js` 60, `features/age.js` 89, `age-kit.js` 41 | `/vote`, `/poll start/reopen`, `/elect`, `/grant`, `/age-declaration`, `GET /decisions.json` | GitHub org token | `KIRA_GOVERNANCE`, `KIRA_ELECTIONS`, `KIRA_AGE_DECLARATION`, `KIRA_GITHUB_ORG_TOKEN`, `KIRA_DECISIONS`, `KIRA_STAFF_LOG_CHANNEL` | Horizon's council and GOVERNANCE.md |
| 38 | **Org chart** | `org-chart.js` 67 | `GET /staff.json` for the hub's People tab | Website | `ORG` | Tiers |
| 39 | **Role grant one-off** | `grant.js` 78, `scripts/grant-videos-role.js` | Give a role to every member, 429-aware | — | — | — |
| 40 | **Measurement scripts** | `scripts/measure-*.mjs` | Replay traffic through AI moderation for cost | Anthropic | — | — |

Not found in Kira: Paddle, a shop, Horizon telemetry. Those live in Horizon's website/game, not in the bot; nothing
to drop on that front. "Sponsors" (#34) is the only money-adjacent feature.

### 1.3 External surfaces Kira depends on

- **Discord**: one guild (`GUILD_ID`), guild-scoped commands only (`bot.js` clears globals), privileged intents
  Server Members + Message Content.
- **GitHub**: fine-grained PAT (issues write) and GitHub App `kira-horizon` (reads, own rate budget); org webhook
  on `horizon-rust` with events `push, pull_request, pull_request_review, issue_comment, issues, check_run,
  check_suite, release`.
- **Railway**: hosting, volume `/data`, `X-Real-IP` for rate limiting, sealed secrets.
- **Anthropic API**: answers, moderation, image classification, bug dedupe (budget `KIRA_AI_DAILY_USD`).
- **Website (playhorizon.dev)**: `POST /api/revalidate` (Kira → site), `GET /github/proxy/*` (site → Kira),
  staff API proxy (site → Kira), `roadmap.json` and `journal.json` (Kira reads).
- **Others**: X API, YouTube Atom feed, the Pumpkin plugin, the SMP mod, 1Password (`op run`) for local runs.

---

## 2. What Sunrise's bot keeps

Context: a new, Sunrise-only server; a team of four (Umut/UMCEKO, Xmilon, Juan Luci/JuanLuci, Ari/arismoko);
repos `sunrise-org/horizon` (branch `release/sunrise`), `sunrise-org/horizon-site` (branch `release/sunrise`,
playsunrise.net), `arismoko/horizon` (mod API work, `main`).

How to read the verbs, given the Rust recommendation in §4:

- **copy as is**: data copied verbatim (regex lists, templates, rule texts, test vectors), with the notice line.
- **copy and adapt**: logic translated to Rust close to line by line, Horizon names and numbers swapped, Kira's
  test cases ported as Rust tests.
- **rewrite**: only the idea is reused; new code.
- **drop**: not built.

If the team picks Node instead (§4.1), "copy and adapt" becomes literal copying with edits.

### 2.1 v1 (first deploy)

| Sunrise feature | Kira source | Verdict | Why / what changes |
|---|---|---|---|
| **Declarative bootstrap** (`server.json`, §3) | `setup.js`, `layout-diff.js`, `permissions.js`, `adopt.js` | copy and adapt | Kira's lookup order (stored id → pinned id → name/`aka`), "stop on two copies", forum tag ids kept by name, Community enabling, and the diff printer are exactly what's needed. Change: Kira is additive-only because its live server drifted from `LAYOUT`; Sunrise starts empty, so the file is authoritative for everything the bot created (plan → apply, empty plan on re-run). Retired/replaced/export logic (`archive.js`) dropped |
| **Bot-owned messages** (welcome, rules, roles buttons, support how-to) | `setup.js` (message upsert), `texts.js` `rolesMessage`/`howToPost`, `adopt.js` `isCopy`/`ensureThread` | copy and adapt | Post once, store id + content hash, edit in place on change, re-post if deleted |
| **Role self-assignment** (Modding, Compat, Site) | `texts.js` `rolesMessage`, `features/community.js` button handler | copy and adapt | Buttons in #roles toggle opt-in roles; a role not marked `self_assign` in `server.json` can never be toggled |
| **GitHub webhook receiver** | `webhook.js`, `notify-kit.js` `signatureOk` | copy and adapt | Same HMAC check, delivery dedupe, 202-then-work, `throttle`. Change: Kira drops deliveries from any owner but `horizon-rust`; Sunrise needs a **repo allowlist** across two owners (`sunrise-org/*`, `arismoko/horizon`) |
| **Site revalidate hook** | `features/site.js` `tagsFor`, `revalidate`, `poke` | copy and adapt | Same call (`POST /api/revalidate`, bearer, `{tags}`), same per-tag 30 s throttle, same warm-up loads. Tag map in config, see §2.4. **Change**: Kira's `pushToMain` matches the repo's *default* branch; `sunrise-org/horizon`'s default branch is currently `perf-platform-upstream`, so match the ref explicitly (`refs/heads/release/sunrise`) |
| **#github-feed** (PRs, releases, issues) | `releases.js` `bullets`/`firstLines`/`bulletField`, `issuekit.js` `neutralise`/`safeTitle`/`forDiscord` | rewrite (formatters copy and adapt) | Kira has no generic feed (it has #releases by polling and #updates from the hub journal). New: one compact embed per event from the webhook payload (PR opened/ready/merged/closed, issue opened/closed, release published, optionally pushes to `release/sunrise`). Release-notes summarising and text neutralising (no pings, no masked links) port from the two kits. Needs **no GitHub token**: everything is in the payload |
| **AutoMod rules** | `automod.js`, `config.js` `MOD.piracy`, `SCAM_PATTERNS`, `TELEGRAM_PATTERNS` | copy as is (patterns) + copy and adapt (applier) | Same Discord AutoMod rules: spam, mention spam, invites/scams, piracy, Telegram. **Review** the piracy pattern "offline mode + bypass": Sunrise legitimately talks about offline mode; keep the TLauncher/SKlauncher/"cracked minecraft"/account-generator patterns, re-test the offline-mode one against Sunrise phrasing before enabling. Port `test/automod.test.js` cases |
| **Spam/flood filter** | `spam-kit.js`, the filter half of `features/moderation.js`, `MOD` numbers | copy and adapt | Flood, repeat, link flood, new-account links, dangerous files, timeout ladder. Keep Kira's tuned numbers (they encode lessons: cases #44/#51/#61/#63). Drop caps/emoji walls unless wanted; drop raid mode for v1 (small server) |
| **Mod commands** | `features/moderation.js` commands | copy and adapt (subset) | `/warn` `/warnings` `/timeout` `/untimeout` `/kick` `/ban` `/purge`, "Report to mods" context menu → #mod-log. Sunrise Team only. No quarantine/appeals |
| **#mod-log** | `core.js` `modlog`, deleted/edited message logging in `features/moderation.js` | copy and adapt | One private log channel (see `server.json`) |
| **#support forum** | `features/forums.js` (`missing`, `withReplies`, `answeredByTeam`, `/done`), `texts.js` `missingMessage`/`howToPost` | copy and adapt | Template check with labels + regex, one pinned "How to post" post, close after a grace period only if nobody from the team answered, `/done` → `solved` tag + archive. Drop `support.js` reply restriction (everyone may help on a small server) and the AI first answer |
| **FAQ** | `config.js` `FAQ`/`FAQ_MORE`, `texts.js` `faqMessages` | rewrite (content), copy and adapt (rendering) | Sunrise's own Q&A (install, which Forge mods work, versions, profiler, how to contribute). `/faq [topic]` with autocomplete answers one entry ephemerally or publicly |
| **Profiler-link helper** | none | rewrite (new) | See §2.3 |
| **Info commands** | `features/community.js` `/help` `/links` | copy and adapt | `/help` and `/links` (playsunrise.net, GitHub repos, /perf, /api-gaps) |
| **Brand** | `brand.js` | copy and adapt | Apply Sunrise's icon/avatar when the file hash changes. Kira's `assets/*` are Horizon's marks: **do not** copy them |
| **Health + config + store** | `store.js`, `core.js`, `bot.js` route table | rewrite | Rust runtime (axum + SQLite). Keep the ideas: one guild, guild-scoped commands, dry-run flags per outbound integration |

### 2.2 Later / optional (behind a flag, off by default)

| Feature | Kira source | Verdict | When |
|---|---|---|---|
| **Contributor role from a verified merged PR** | `features/contributors.js`, `contributor-kit.js` (`/link-github`, `/verify-github`) | copy and adapt | Soon after v1: DEVELOPMENT is writable by Contributor+, so automating the role matters once outsiders show up. Count merged PRs in `sunrise-org/horizon`, `sunrise-org/horizon-site`, `arismoko/horizon`. Drop ore ranks and the one-off clean-up. Needs a GitHub App (search API) |
| **Polls** (Xmilon: "maybe future") | `poll-kit.js` (count rules only) | copy and adapt (subset) | Discord native polls already exist; if the team wants recorded decisions, port simple-majority/2-3 counting and the 24 h close, without councils, recusal or vetoes |
| **Honeypot** | `honeypot-kit.js`, handler in `features/moderation.js` | copy and adapt | When scam bots appear; 52-line kit, cheap |
| **Issue from #support** | `features/github.js`, `issuekit.js` | copy and adapt | When a support post turns out to be a bug: `/issue` (team) files it in `sunrise-org/horizon` with the thread link; answers relayed back. Needs a GitHub App with Issues: write |
| **`/claim` from Discord** | `features/workboard.js` (claim part), `workkit.js` | copy and adapt | Only if contributors want it. The game repo already has GitHub-side `/claim` (`.github/workflows/claim-command.yml`; issue_comment workflows run from the default branch, currently `perf-platform-upstream`: check it carries the file) |
| **Notify DMs** | `features/notify.js`, `notify-kit.js` | copy and adapt | When there are outside contributors with open PRs |
| **Mod-mail** | `features/community.js` mod-mail part | copy and adapt | When there are moderators beyond the team |
| **AI answers** | `features/ai.js`, `ai.js`, `ai-kit.js` | copy and adapt | Optional; needs an Anthropic key and budget. Keep `checkReply()` (link allowlist, no pings), the `<member_message>` tag defusing, the daily budget and hand-offs. Sources: Sunrise docs on `release/sunrise` |
| **Mod cases / quarantine / appeals** | `features/modcases.js`, `modcase-kit.js` | copy and adapt | Only if the server grows a mod team |
| **Release posts with a daily ping** | `releases.js` `pingDecision` | copy and adapt | `sunrise-org/horizon` has no releases yet; v1 posts releases in #github-feed; a separate #releases + ping role once releases are regular |
| **GitHub proxy for the site** | `features/site.js` `routes`/`proxyTarget` | drop for now | The `site` Deployment already has its own `GITHUB_TOKEN` (`site-github` Secret) and no `GITHUB_PROXY_URL` set, so it doesn't need one. Revisit only if the site hits rate limits |

### 2.3 Profiler-link helper (new)

People paste `https://playsunrise.net/perf/c/<id>` links (also `perf.playsunrise.net/...`, which redirects).

- Match `https?://(perf\.)?playsunrise\.net/perf/c/([a-z2-7]{10})` (the site's `CAPTURE_ID`,
  `lib/perf/sharing.ts`) in any message the bot can read, at most 3 links per message.
- Fetch `GET https://playsunrise.net/api/perf/captures/<id>/meta` (public; returns `CaptureMetadata`:
  `id, bytes, created_utc, expires_utc, shared, summary { kind, seconds, version }`).
- Reply once (no ping, `allowed_mentions` empty) with a small embed: kind, duration, Sunrise version, size,
  "expires in N days" or "shared (kept)", and the link. 404 → "this capture expired or doesn't exist".
- In #support posts tagged `performance` with no capture link, the template notice asks for one and links
  playsunrise.net/perf (how to record a capture).
- `/perf` command: how to record and share a capture (text in config), plus `/perf compare a:<id> b:<id>` linking
  the viewer's comparison view if the site supports a URL for it (check with the site; otherwise omit).
- Throttle: one meta fetch per capture id per 10 minutes (cache in memory).

### 2.4 Revalidate tag map (config, not code)

From `sunrise-org/horizon-site` `components/github-fresh.ts` on `release/sunrise`:

| Delivery | Tag |
|---|---|
| `push` to `sunrise-org/horizon` `refs/heads/release/sunrise` | `github:sunrise-org/horizon:release/sunrise` |
| `issues` / `pull_request` on `sunrise-org/horizon`, actions opened, closed, reopened, edited, labeled, unlabeled, ready_for_review, converted_to_draft, deleted, transferred (Kira's `ACTIONS_THAT_SHOW`) | `github:sunrise-org/horizon:issues` |
| `release` on `sunrise-org/horizon` | `github:sunrise-org/horizon:releases` |
| `push` to `arismoko/horizon` `refs/heads/main`, and `issues` on `arismoko/horizon` (the site's comment: "main and issues", for /api-gaps) | `github:arismoko-horizon:main` |
| `push` to `sunrise-org/horizon-mods` `main` (repo doesn't exist yet: keep the row, dormant) | `github:sunrise-org/horizon-mods:main` |

Warm-up paths per tag (Kira's `SITE.warm`) must be re-listed for playsunrise.net's pages (e.g. `/`, `/docs`,
`/api-gaps`); confirm with the site.

Prerequisite on the site side: the `site` Deployment currently has **no `REVALIDATE_SECRET`** (its ConfigMap `site`
and Secret `site-github` don't set it), so `/api/revalidate` answers 404 today. Add it to the site (same value as
the bot's `SITE_REVALIDATE_SECRET`).

### 2.5 Dropped

| Kira feature | Why |
|---|---|
| Governance, council, elections, team votes, 18+ declaration (`poll.js`, `elections*.js`, `governance-kit.js`, `decisions-kit.js`, `age*.js`, `teamvote.js`, `teamroles.js`) | Horizon's GOVERNANCE.md; a four-person team decides in chat |
| Applications, staff dashboards, org chart (`staff-*.js`, `applications.js`, `application-votes.js`, `apply*.js`, `team-data.js`, `team-stats.js`, `org-chart.js`) | Tied to playhorizon.dev/hub/team; no recruiting process yet |
| Batch/image AI moderation (`batchmod.js`, `batch-kit.js`, `image-kit.js`) | Cost and tuning for a large server; AutoMod + filters suffice |
| Hub front desk, roadmap, #updates, journal (`hub*.js`, `updates.js`, `journal.js`) | Built on playhorizon.dev's roadmap/journal, which Sunrise doesn't publish |
| Work board, MCP agents (`board.js`, `workkit.js`, `workboard.js`, `mcp.js`, `agents*.js`, `scripts/board.mjs`) | GitHub itself is the board for four people |
| Triage via bot (`triage*.js`) | The team has write access |
| Sponsors, SMP market, game chat, X, YouTube (`sponsors*.js`, `smp*.js`, `game*.js`, `x*.js`, `videos.js`) | Horizon/Ludovic-specific integrations (Pumpkin plugin, SMP mod, Ludovic's sponsors and channels) |
| Levels, voice rooms, starboard, reminders, chat clears (`levels.js`, `rooms.js`, `community.js` parts, `chatclear.js`) | Engagement features a small dev-focused server doesn't need; easy to add later |
| Launcher GitHub fallback (`launcher-github.js`) | Sunrise's launcher doesn't call it |
| Railway sealing, admin backup/restore (`secrets.js`, `seal.mjs`, `admin.js`) | Kubernetes Secrets and PVC snapshots replace them |
| Brand assets (`assets/*`) | Horizon's marks |

---

## 3. Server bootstrap

### 3.1 Behaviour

`sunrise-bot bootstrap` reads `config/server.json`, reads the live guild, and computes a **plan** (pure function,
no network, ported from `layout-diff.js`):

- `sunrise-bot bootstrap` (default `--plan`): prints the plan, changes nothing.
- `sunrise-bot bootstrap --apply`: applies it, saving each created id to the store **immediately** (Kira's
  lesson: a crash must not create a second copy).
- Running `--apply` again prints "nothing to do" and makes **zero write calls**. A test asserts this.

Finding objects (from `layout-diff.js` `findChannel`/`findCategory`): stored id → `id` pinned in the file → exact
name, name without emoji decoration, or a name in `aka`; two matches by name → stop with an error.

What the file owns, and is reconciled on `--apply`: role name/colour/hoist/mentionable/permissions/order;
category name/order/overwrites; channel name/type/parent/topic/order/overwrites/slowmode; forum tags (edited by
name so existing tag ids, and the tags on existing posts, survive: `setup.js` keeps `existing.find(t => t.name ===
name)`), forum guidelines, default layout and reactions; Community settings; bot-owned messages.

What it never does: delete or edit anything it did not create (ids not in its store), touch member role
assignments (except giving the Sunrise Team role to the ids in `team_members`), or delete a channel that was
removed from the file (it prints "not in the file anymore: delete by hand", or deletes with `--prune` only if the
bot created it).

Community mode: forums and announcement channels need it; it needs a rules channel, a community-updates channel,
verification ≥ low and the explicit-content filter on all members (Kira's `enableCommunity`). Bootstrap enables
Community first, then creates forums and the announcement channel.

Not doable by a bot: Discord's Server Guide ("Bots cannot use this endpoint", Kira README); the bot prints
the wanted text for a human to paste. Onboarding prompts and rules screening can be set by API (Kira does both);
leave them out of v1 and add when the server opens publicly.

### 3.2 `server.json`

Access presets (closed set, an enum in Rust):

- `public`: everyone reads and writes.
- `readonly`: everyone reads; Sunrise Team and the bot write.
- `bot_feed`: everyone reads; only the bot writes.
- `contributors_write`: everyone reads and reacts; Contributor and Sunrise Team write, open posts/threads and
  reply in them.
- `team_private`: only Sunrise Team (and the bot) see it.

A category's access applies to its channels; a channel may set its own (channels are created with explicit
overwrites, never "synced", so a later category edit can't silently change them).

```json
{
  "$schema": "./server.schema.json",
  "version": 1,
  "guild": {
    "name": "Sunrise",
    "verification_level": "medium",
    "explicit_content_filter": "all_members",
    "default_notifications": "only_mentions",
    "community": { "rules_channel": "rules", "updates_channel": "team" },
    "everyone_permissions": [
      "ViewChannel", "ChangeNickname", "SendMessages", "SendMessagesInThreads", "EmbedLinks", "AttachFiles",
      "AddReactions", "UseExternalEmojis", "ReadMessageHistory", "UseApplicationCommands", "Connect", "Speak", "Stream", "UseVAD"
    ]
  },
  "roles": [
    { "key": "team", "name": "Sunrise Team", "color": "#F28C28", "hoist": true, "mentionable": true,
      "permissions": ["ManageMessages", "ManageThreads", "ModerateMembers", "KickMembers", "BanMembers", "MentionEveryone", "ManageNicknames"] },
    { "key": "contributor", "name": "Contributor", "color": "#4FB37A", "hoist": true, "mentionable": false, "permissions": [] },
    { "key": "modding", "name": "Modding", "color": "#7A6FF0", "self_assign": true, "description": "Mod API and mod porting talk: pinged for #mod-api news", "permissions": [] },
    { "key": "compat", "name": "Compat", "color": "#3FA7D6", "self_assign": true, "description": "Forge/Fabric compatibility and versions: pinged for compat news", "permissions": [] },
    { "key": "site", "name": "Site", "color": "#E05C7A", "self_assign": true, "description": "playsunrise.net and the profiler viewer", "permissions": [] }
  ],
  "role_order": ["team", "contributor", "modding", "compat", "site"],
  "team_members": { "UMCEKO": "<discord id>", "Xmilon": "<discord id>", "JuanLuci": "<discord id>", "arismoko": "<discord id>" },
  "categories": [
    {
      "key": "start", "name": "START HERE", "access": "readonly",
      "channels": [
        { "key": "welcome", "name": "welcome", "type": "text", "topic": "What Sunrise is and where everything is.", "messages": ["welcome"] },
        { "key": "rules", "name": "rules", "type": "text", "topic": "The rules.", "messages": ["rules"] },
        { "key": "announcements", "name": "announcements", "type": "announcement", "topic": "Sunrise news." },
        { "key": "roles", "name": "roles", "type": "text", "topic": "Pick the roles you want pings for.", "messages": ["role_buttons"] }
      ]
    },
    {
      "key": "community", "name": "COMMUNITY", "access": "public",
      "channels": [
        { "key": "general", "name": "general", "type": "text", "topic": "Talk about Sunrise." },
        { "key": "showcase", "name": "showcase", "type": "text", "topic": "Screenshots, videos and builds made with Sunrise.", "slowmode_seconds": 60 },
        { "key": "offtopic", "name": "off-topic", "type": "text", "topic": "Anything else." }
      ]
    },
    {
      "key": "help", "name": "HELP", "access": "public",
      "channels": [
        {
          "key": "support", "name": "support", "type": "forum",
          "topic": "Help with Sunrise: installing, crashes, mods, performance. One problem per post, with the template.",
          "require_tag": true, "layout": "list", "sort": "latest_activity",
          "tags": [
            { "emoji": "📦", "name": "install" },
            { "emoji": "💥", "name": "crash" },
            { "emoji": "🧩", "name": "mods" },
            { "emoji": "⚡", "name": "performance" },
            { "emoji": "✅", "name": "solved", "team_only": true }
          ],
          "done_tag": "solved",
          "template": {
            "text": "**What's wrong:**\n**What I tried:**\n**Sunrise version:**\n**Mods (and their versions):**\n**OS, CPU, GPU:**\n+ the log, a crash report, or a playsunrise.net/perf link for performance",
            "min_chars": 40,
            "needs": [
              { "label": "What's wrong", "regex": "(?i)what'?s wrong|wrong|problem|issue" },
              { "label": "OS, CPU, GPU", "regex": "(?i)\\b(os|windows|linux|mac|macos|gpu|nvidia|amd|intel|radeon|geforce)\\b" }
            ],
            "needs_by_tag": { "performance": [{ "label": "a playsunrise.net/perf link", "regex": "playsunrise\\.net/perf/c/[a-z2-7]{10}" }] },
            "grace_minutes": 60
          },
          "messages": ["support_howto"]
        },
        { "key": "profiler", "name": "profiler", "type": "text", "topic": "Share playsunrise.net/perf captures and read them together." }
      ]
    },
    {
      "key": "dev", "name": "DEVELOPMENT", "access": "contributors_write",
      "channels": [
        { "key": "modapi", "name": "mod-api", "type": "text", "topic": "The mod API (arismoko/horizon)." },
        { "key": "forgecompat", "name": "forge-compat", "type": "text", "topic": "Running Forge mods on Sunrise." },
        { "key": "fabriccompat", "name": "fabric-compat", "type": "text", "topic": "Fabric support (later)." },
        { "key": "versioning", "name": "versioning", "type": "text", "topic": "Multi-version support." },
        { "key": "site", "name": "site", "type": "text", "topic": "playsunrise.net and the profiler viewer." },
        {
          "key": "ideas", "name": "ideas", "type": "forum", "topic": "One idea per post.", "require_tag": true, "layout": "list", "sort": "creation_date",
          "default_reaction": "👍",
          "tags": [
            { "emoji": "🧩", "name": "mod api" }, { "emoji": "🔧", "name": "compat" }, { "emoji": "🕰️", "name": "versions" },
            { "emoji": "⚡", "name": "profiler" }, { "emoji": "🌐", "name": "site" }, { "emoji": "💡", "name": "other" },
            { "emoji": "📌", "name": "planned", "team_only": true }, { "emoji": "✅", "name": "done", "team_only": true }, { "emoji": "❌", "name": "won't do", "team_only": true }
          ],
          "done_tag": "done"
        },
        { "key": "githubfeed", "name": "github-feed", "type": "text", "access": "bot_feed", "topic": "Pull requests, issues and releases from sunrise-org and arismoko/horizon." }
      ]
    },
    {
      "key": "team", "name": "TEAM", "access": "team_private",
      "channels": [
        { "key": "team", "name": "team", "type": "text", "topic": "Team chat." },
        { "key": "reviews", "name": "reviews", "type": "text", "topic": "Review requests and review talk." },
        { "key": "planning", "name": "planning", "type": "text", "topic": "What's next." },
        { "key": "modlog", "name": "mod-log", "type": "text", "access": "bot_feed", "visible_to": "team", "topic": "What the bot removed, edited messages, mod actions." }
      ]
    }
  ],
  "messages": {
    "welcome": { "embed": { "title": "Welcome to Sunrise", "description": "Sunrise is a community fork of Horizon (Minecraft: Java Edition 26.3 in Rust) that runs Java mods (Forge now, Fabric later), several Minecraft versions, and ships a built-in profiler.\n\n#support for help · #profiler for performance · #roles for pings · https://playsunrise.net" } },
    "rules": { "embed": { "title": "Rules", "description": "1. Be kind.\n2. No spam or self-promotion.\n3. No piracy: Sunrise needs your own copy of Minecraft: Java Edition.\n4. Keep it SFW.\n5. English in public channels." } },
    "role_buttons": { "role_buttons": ["modding", "compat", "site"] },
    "support_howto": { "forum_post": { "title": "📌 How to post here: read me first", "pinned": true, "locked": true, "body_from": "template" } }
  }
}
```

Notes on choices:

- `#mod-log` is **added** to TEAM (moderation needs somewhere to log); it is bot-written, team-visible.
- "Contributor+" = Contributor and Sunrise Team. Modding/Compat/Site are opt-in ping roles; they grant no write
  access. If the team meant them as area-team roles that should write in DEVELOPMENT, add them to the preset.
- `#ideas` follows its category (`contributors_write`), so only contributors open ideas. If ideas should be open
  to everyone, give it `"access": "public"`.
- `#announcements` is an announcement channel so other servers can follow it.
- Community's required "updates" channel points to `#team` (Discord's notices go to the team, not the public).
- Validation at load (`serde(deny_unknown_fields)` + checks): unique keys, every referenced key exists, at most 20
  forum tags, names within Discord limits, regexes compile, `done_tag` is a tag of that forum.

---

## 4. Architecture

### 4.1 Language: Rust (recommended) vs Kira's JavaScript

| | Rust | Fork Kira (Node) |
|---|---|---|
| Team | Writes Rust daily; one toolchain with the game | New stack to maintain for a 4-person Rust team |
| Reuse of Kira | Data and test vectors verbatim; logic ported (the v1 subset is ~1.5–2k lines of Kira) | Literal copy, but out of ~35k lines ~80 % is deleted and `config.js` (991 lines of Horizon ids/texts) rewritten anyway |
| Types | serde with `deny_unknown_fields`, enums for presets, events, tags: bad config fails at start | zod at the edges only; Kira's state is untyped JSON |
| Discord coverage | serenity 0.12: slash commands (via poise), forums with tags, AutoMod rules, threads, components; raw `Http` request for anything missing (Community settings, Onboarding if ever) | discord.js covers everything; Kira proves every call |
| Ops | ~15 MB static binary, distroless image, low memory | Node 22 image, ~100 MB |
| Time to v1 | Longer (port + tests) | Shorter (delete + edit) |

Recommendation: **Rust**. The v1 subset is small, the valuable parts of Kira are its rules and lessons (which port
cleanly because they sit in Discord-free kits), and the team will maintain it. Accept a longer first PR series.
Check early in step 3 that serenity can do the Community/forum calls; fall back to `serenity::http::Request` raw
calls where it can't (the same JSON Kira sends).

Stack:

- `serenity` 0.12 + `poise` (commands, components), gateway intents: Guilds, GuildMembers, GuildMessages,
  MessageContent, GuildModeration (no voice, reactions, invites or DMs in v1).
- `axum` for HTTP: `POST /github/webhook`, `GET /healthz`, `GET /readyz`.
- `tokio`, `reqwest` (rustls), `hmac` + `sha2` (signature), `subtle` (constant-time compare), `serde`/`serde_json`.
- `sqlx` with SQLite and embedded migrations.
- `regex` (AutoMod-compatible patterns are tested with it, like Kira's tests check JS regexes behave like Discord's).
- `tracing` to stdout (JSON).

Code layout (one binary, two subcommands `run` and `bootstrap`):

```
sunrise-bot/
  config/server.json, config/bot.toml
  src/main.rs                 cli: run | bootstrap [--apply] [--prune]
  src/config/                 typed server.json + bot.toml, validation
  src/kit/                    no Discord, no network: layout plan, permissions, webhook (signature, routing), revalidate tags, feed formatting, spam verdicts, forum template check, perf link parsing
  src/discord/                serenity glue: bootstrap executor, commands, handlers
  src/http/                   axum routes
  src/store/                  sqlite + migrations
  LICENSE                     Sunrise's MIT
  LICENSES/kira-MIT.txt       Kira's licence, verbatim
```

Each ported file starts with one line:
`// Ported from Kira (github.com/horizon-rust/kira, src/<file>.js), MIT © 2026 Ludovic Marie and the Kira contributors; see LICENSES/kira-MIT.txt`.
Data copied verbatim (regex lists, test vectors) carries the same line above it.

### 4.2 Hosting (umceko cluster, namespace `sunrise`)

Mirrors the existing `site` Deployment (`horizon-site/deploy/*.yaml`: image `hub.umceko.com/sunrise/site@sha256`,
`runAsNonRoot`, uid/gid 1000, local-path PVCs, Gateway `sunrise`).

- **Image**: `hub.umceko.com/sunrise/bot:<sha>`; deploy pins the digest (`kubectl -n sunrise set image deploy/bot
  bot=<digest>`), as the site does.
- **Deployment** `bot`: 1 replica, `strategy: Recreate` (one gateway session, one SQLite writer), non-root
  1000:1000, read-only root filesystem, resources ~50m/64Mi requests.
- **PVC** `bot-data` 256Mi `local-path`, mounted at `/data` (`/data/bot.sqlite`).
- **Service** `bot` port 8080; **HTTPRoute** `bot.playsunrise.net` on Gateway `sunrise` listener `subdomains`
  (exact hostnames win over the existing `*.playsunrise.net` redirect route). Only `/github/webhook` and
  `/healthz` exposed; everything else 404.
- **Probes**: liveness `/healthz` (process up), readiness `/readyz` (gateway connected, DB open).
- **Manifests** in the bot repo's `deploy/` (namespace already exists), applied with `kubectl apply -k deploy/`.
- **Backup**: copy `/data/bot.sqlite` with `sqlite3 .backup` from a CronJob into `rustfs` (already in the
  namespace), or rely on the fact that every Discord object can be re-found by name: losing the DB costs only
  message ids and moderation history.

### 4.3 Storage (SQLite)

| Table | Holds |
|---|---|
| `discord_objects(kind, key, id, hash)` | ids of roles, categories, channels, bot-owned messages, forum how-to posts; `hash` of the last applied content |
| `webhook_deliveries(id, at)` | last 2,000 `X-GitHub-Delivery` ids (Kira keeps them in memory; persisting survives restarts) |
| `feed_posted(source, ref, message_id, at)` | events already posted to #github-feed (e.g. `release:<id>`, `pr:<repo>#<n>:merged`) |
| `warnings(user_id, by, reason, at)`, `timeouts(user_id, at, length)` | moderation ladder and `/warnings` |
| `forum_checks(thread_id, forum, owner, at, notice_id)` | template notices waiting for the grace period |
| `revalidations(tag, at, status)` | last call per tag, for `/status` and debugging |

Only the bot process writes. `bootstrap` runs as a one-shot inside the same pod (`kubectl exec deploy/bot --
sunrise-bot bootstrap --apply`) so it uses the same DB file; no Kira-style split of keys between two writers.

### 4.4 Secrets and configuration

- **Kubernetes Secret** `bot` (created by hand with `kubectl create secret`, like `site-github`; never in git):
  `DISCORD_TOKEN`, `GITHUB_WEBHOOK_SECRET`, `SITE_REVALIDATE_SECRET` (same value as the site's new
  `REVALIDATE_SECRET`). Later: `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY` (contributor verification, issue filing),
  `ANTHROPIC_API_KEY` (AI answers).
- **ConfigMap** / env: `GUILD_ID`, `DATABASE_PATH=/data/bot.sqlite`, `HTTP_ADDR=0.0.0.0:8080`, `SITE_URL=https://playsunrise.net`,
  dry-run switches `SITE_DRY_RUN`, `FEED_DRY_RUN` (log instead of calling out, as Kira's `KIRA_*_DRY_RUN`).
- **In git** (baked into the image): `config/server.json` (structure, texts) and `config/bot.toml` (repo allowlist,
  tag map, warm paths, feed filters, spam thresholds, AutoMod patterns, FAQ). Every feature has an `enabled` flag
  there; off by default except the v1 set.
- **Discord app**: a new application "Sunrise" owned by a team account or a Discord Developer Team with the four
  members, Server Members + Message Content intents, invited with the permissions bootstrap needs (Manage Roles,
  Manage Channels, Manage Guild for Community/AutoMod, Moderate Members, Kick, Ban, Manage Messages, Manage
  Threads). Not Administrator after bootstrap: re-invite with the narrower set once the layout exists.
- **GitHub webhooks**: one org webhook on `sunrise-org` (events `push, pull_request, issues, release`; add
  `issue_comment`, `pull_request_review` later for notify) → `https://bot.playsunrise.net/github/webhook`; one repo
  webhook on `arismoko/horizon` (Ari creates it, same secret). Secret piped from the Secret, never on a command line
  (Kira README "Setting the webhook up").

### 4.5 Tests

- Kit tests (`cargo test`, no network, no tokens), ported from Kira where a kit is ported:
  - `test/webhook.test.js` → signature valid/invalid/missing, duplicate delivery, foreign repo dropped, throttle gaps.
  - `features/site.js` tests → tag map per delivery (including "push to default branch ≠ release/sunrise").
  - `test/setup-additive.test.js`, `test/fixtures/live-channels-*.json` style → planner: empty guild → full plan;
    apply plan to a fake guild → re-plan is empty; renamed channel found by `aka`; duplicate names → error;
    forum tag edit keeps ids; never touches unknown ids.
  - `test/automod.test.js` → the copied piracy/scam/Telegram patterns: must-match and must-not-match lists
    (add Sunrise's "offline mode" phrases to must-not-match).
  - `test/spam.test.js` → flood/repeat/cross-post verdicts.
  - `test/forum-template.test.js` → template `missing()` and `withReplies()`.
  - `test/releases.test.js` → release-notes bullets for the feed.
  - New: perf link parsing (ids, max 3, perf. subdomain), meta → embed, 404.
- HTTP tests with axum's `Router` in-process: signed sample payloads (GitHub's documented examples) → expected
  revalidate calls and feed posts through a fake Discord/HTTP sink.
- Live checks on a private test guild with a second bot token, never on the real server, before each deploy that
  touches bootstrap.
- CI on `sunrise-org/sunrise-bot`: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, image build.

---

## 5. Implementation plan

Each step is one PR (< ~500 lines). Repo: a new `sunrise-org/sunrise-bot` (to be created by the team).

| # | Step | Copies from Kira |
|---|---|---|
| 1 | Scaffold: cargo crate, `run`/`bootstrap` CLI, tracing, `LICENSE` + `LICENSES/kira-MIT.txt`, CI, Dockerfile (distroless, uid 1000) | `LICENSE` text verbatim; `.github/workflows/test.yml` shape |
| 2 | Config types: `server.json` + `bot.toml` schemas, validation, the `server.json` of §3.2 with placeholders for Discord ids | `config.js` `LAYOUT` field set (adapted), forum `check` shape |
| 3 | Bootstrap planner (pure): live snapshot → plan; permission presets → overwrites; tests including "second plan is empty" | `layout-diff.js` (`findChannel`, `findCategory`, `layoutDiff`, `rolesDiff`, `overwriteLines`), `permissions.js` (`readonly`, `writers`, `staff` branches → presets), `adopt.js` (`unique`) |
| 4 | Bootstrap executor + store: roles, Community, categories, channels, forum tags; ids saved on creation; spike serenity's coverage first | `setup.js` (`ensureRoles`, `ensureChannels`, `enableCommunity`, forum tag id preservation), `store.js` (save-at-once idea) |
| 5 | Bot-owned messages: welcome, rules, role buttons, support how-to post (pinned, locked); upsert by hash | `setup.js` message upsert, `texts.js` (`rolesMessage`, `howToPost`), `adopt.js` (`isCopy`, `ensureThread`) |
| 6 | Runtime skeleton: gateway, guild-scoped commands, `/help` `/links`, role-button handler, `/healthz` `/readyz`; k8s manifests (Deployment, PVC, Service, Secret ref, HTTPRoute); first deploy; run bootstrap on the real server | `bot.js` (guild commands only, globals cleared), `features/community.js` (`/help`, `/links`, buttons), `Dockerfile`/`railway.json` health idea |
| 7 | GitHub webhook receiver: HMAC, dedupe (persisted), repo allowlist, typed events, throttle | `webhook.js` (`routes`, `throttle`, `healthy`), `notify-kit.js` `signatureOk`; `test/webhook.test.js` cases |
| 8 | Site revalidate: tag map from config, per-tag throttle, warm paths, dry run; add `REVALIDATE_SECRET` to the site Deployment | `features/site.js` (`tagsFor`, `revalidate`, `poke`, `ACTIONS_THAT_SHOW`) |
| 9 | #github-feed: PR opened/ready/merged/closed, issues opened/closed, releases; neutralised text, no pings, de-duplicated | `releases.js` (`bullets`, `firstLines`, `bulletField`), `issuekit.js` (`neutralise`, `safeTitle`, `forDiscord`) |
| 10 | AutoMod rules from config (piracy, scams, Telegram, mention spam, invites) with team exemption; pattern tests | `automod.js` (`automodRules`, `applyAutomod`, `keywordRegex`, `patternRegex`), `config.js` `MOD.piracy` + `SCAM_PATTERNS` + `TELEGRAM_PATTERNS` verbatim (offline-mode pattern reviewed); `test/automod.test.js` |
| 11 | Spam/flood filter + timeout ladder + #mod-log (deletes, edits) | `spam-kit.js`, filter part of `features/moderation.js`, `MOD` thresholds, `core.js` `modlog`; `test/spam.test.js` |
| 12 | Mod commands: `/warn` `/warnings` `/timeout` `/untimeout` `/kick` `/ban` `/purge`, "Report to mods" | `features/moderation.js` command handlers, `core.js` `parseDuration`/`human` |
| 13 | #support: template check, notices, grace-period close unless the team answered, `/done` | `features/forums.js` (`missing`, `withReplies`, `answeredByTeam`, `inspect`, `/done`), `texts.js` `missingMessage`; `test/forum-template.test.js` |
| 14 | `/faq` with autocomplete, FAQ in `bot.toml` | `texts.js` `faqMessages` rendering; content new |
| 15 | Profiler-link helper + `/perf` + performance-tag nudge in #support | new; `CAPTURE_ID` from the site |
| 16 | Brand: Sunrise icon/avatar applied on hash change | `brand.js` |
| later | Contributor verification + auto role; honeypot; polls; issue from #support; notify DMs; mod-mail; AI answers | `contributors.js`/`contributor-kit.js`; `honeypot-kit.js`; `poll-kit.js`; `features/github.js`/`issuekit.js`; `notify*.js`; `community.js`; `ai*.js` |

Order rationale: bootstrap first (the server is empty and everything else needs its channels); then the
integrations the team asked for (feed, revalidate); then moderation before the server is opened to the public;
support and profiler help last, because they matter once players arrive.

### Open questions for the team

1. Are Modding/Compat/Site opt-in ping roles (assumed) or area-team roles with write access in DEVELOPMENT?
2. Should `#ideas` be open to everyone or Contributor+ (assumed Contributor+, as its category)?
3. Feed scope: every PR event or only opened/merged? Pushes to `release/sunrise` too?
4. Where do Discord's Community notices go (assumed #team)?
5. Who owns the Discord application (one person or a Developer Team)?
6. Is the game repo's default branch going back to `main`? It changes where GitHub-side `/claim` runs.

---

## 6. Summary

**Keep (v1):** declarative bootstrap (from `setup.js`/`layout-diff.js`/`permissions.js`), bot-owned messages and
role buttons, GitHub webhook receiver (`webhook.js`), site revalidate (`features/site.js`, with Sunrise's tags and an
explicit `release/sunrise` ref), a new #github-feed (formatters from `releases.js`/`issuekit.js`), AutoMod patterns
(`automod.js`, offline-mode rule reviewed) and spam filter (`spam-kit.js`), a small set of mod commands and #mod-log,
#support template check and `/done` (`features/forums.js`), static `/faq`, `/help` `/links`, brand, and a new
profiler-link helper reading `playsunrise.net/api/perf/captures/<id>/meta`.

**Later (flagged):** contributor verification and auto Contributor role, polls (simple, no governance), honeypot,
issue filing from #support, `/claim`, notify DMs, mod-mail, AI answers, mod cases/appeals, a separate #releases.

**Drop:** governance/council/elections/team votes/18+, applications and staff dashboards, AI batch and image
moderation, hub/roadmap/#updates/journal, work board and MCP agents, triage-by-bot, sponsors, SMP market, game chat
(Pumpkin), X, YouTube, levels, voice rooms, starboard, chat clears, launcher GitHub fallback, the GitHub proxy
(the site already has its own token), Railway sealing and admin routes, Horizon's brand assets.

**Language:** Rust (serenity + poise, axum, SQLite), one Deployment in the `sunrise` namespace beside `site`.
Kira's MIT licence lets Sunrise reuse its patterns, regexes, texts and test cases verbatim; the logic is ported
module by module with a one-line notice and `LICENSES/kira-MIT.txt`.
