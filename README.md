# Hocuspanda

A Wayland desktop for developers who work with AI agents: see at a glance
what your agents are doing; everything else beautiful, fluid and out of
the way.

A compositor on smithay, a shell drawn with iced (the bar, its popovers,
the launcher, notifications, the lock screen) and a Settings application.
Claude Code, Codex and herdr are followed out of the box; any other tool
tells the bar its state with one command ([docs/agents.md](docs/agents.md)).

**Version 1.0.0**: what it holds, its requirements and its known limits
are in [the release notes](docs/releases/1.0.0.md).

## Install

On Debian testing (amd64), download the `.deb` from the
[releases](https://github.com/mattboll/hocus-panda/releases), then:

```sh
sudo apt install ./hocuspanda_1.0.0-1_amd64.deb
```

Log out and choose the "Hocuspanda" session in the display manager (GDM,
SDDM, greetd's greeters). `Super+Return` opens a terminal, `Super+Space`
the launcher, `Super+P` every action, `Super+Shift+E` logs out. If the
session does not start, the display manager comes back and
`journalctl --user -t hocuspanda -b` says why.

Its configuration is `~/.config/hocuspanda/config.toml`, written by the
Settings; [packaging/config.toml](packaging/config.toml) lists every
setting with its default. To build the package yourself, see
[packaging/README.md](packaging/README.md).

## Documentation

- [docs/vision.md](docs/vision.md): what Hocuspanda is for, and what it
  is not.
- [docs/design/](docs/design/): each part's spec and how it was built,
  starting with [the bar](docs/design/bar.md),
  [windows](docs/design/windows.md) and
  [the Settings](docs/design/settings.md).
- [docs/decisions/](docs/decisions/) and
  [docs/design/tech-choices.md](docs/design/tech-choices.md): the
  technical choices.
- [CLAUDE.md](CLAUDE.md): how to build, test and run it, for people and
  agents working on it.

The first build, before the restart from the vision and the design, is
kept under the tag `v0-first-build`.

## Licence

MIT ([LICENSE](LICENSE)). The fonts the shell carries (Martian Mono,
Manrope) are under the SIL Open Font License.
