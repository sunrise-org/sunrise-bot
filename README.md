# Sunny

Sunrise's Discord bot. The design is in [SPEC.md](SPEC.md).

```sh
sunrise-bot bootstrap            # print what would change on the server (DISCORD_TOKEN, GUILD_ID)
sunrise-bot bootstrap --apply    # make it so; --prune also deletes channels the bot made that left the file
sunrise-bot run                  # the bot: gateway, /help, /links, role buttons, /healthz, /readyz
```

Environment: `DISCORD_TOKEN`, `GUILD_ID`, and optionally `DATABASE_PATH` (default `sunrise-bot.sqlite`), `CONFIG_DIR` (default `config`), `HTTP_ADDR` (default `0.0.0.0:8080`).

`config/server.json` is the server (roles, categories, channels, messages); `config/bot.toml` is what the bot says.

Deploy: `deploy/bot.yaml` (Deployment, PVC, Service, ConfigMap; the token is in the Secret `bot`), applied with `kubectl -n sunrise apply -f deploy/`. Run the bootstrap in the pod: `kubectl -n sunrise exec deploy/bot -- sunrise-bot bootstrap`.

Licence: [PolyForm Shield 1.0.0](LICENSE). Versions up to and including `03631834e7` were released under MIT,
and those releases stay MIT. Parts are ported from [Kira](https://github.com/horizon-rust/kira) and keep Kira's MIT
licence; see `LICENSES/kira-MIT.txt`.
