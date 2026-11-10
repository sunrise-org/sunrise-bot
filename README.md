# Sunny

Sunrise's Discord bot. The design is in [SPEC.md](SPEC.md).

```sh
cargo run -- bootstrap           # print the plan for the server (needs DISCORD_TOKEN and GUILD_ID)
cargo run -- bootstrap --apply   # make it so
cargo run -- run                 # the bot
```

Parts are ported from [Kira](https://github.com/horizon-rust/kira) (MIT); see `LICENSES/kira-MIT.txt`.
