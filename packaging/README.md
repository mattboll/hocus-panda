# Packaging

What the package of Hocuspanda holds, and how it is built.

## What is installed

| File | Where | Why |
|---|---|---|
| `hocuspanda` | `/usr/bin` | The compositor |
| `hocuspanda-shell` | `/usr/bin` | The shell (the bar), which the compositor starts and restarts |
| [`hocuspanda-session`](hocuspanda-session) | `/usr/bin` | What display managers run: once through the user's login shell, then `hocuspanda --session`; it ends when the compositor does, however it ends, so the display manager comes back |
| [`hocuspanda.desktop`](hocuspanda.desktop) | `/usr/share/wayland-sessions` | Display managers (GDM, SDDM, greetd's greeters) offer the "Hocuspanda" session |
| [`config.toml`](config.toml) | `/usr/share/doc/hocuspanda` | Every setting with its default, to copy to `~/.config/hocuspanda/` |
| `LICENSE` | `/usr/share/doc/hocuspanda/copyright` | MIT, and the SIL Open Font License of the fonts the shell carries in its binary (Martian Mono, Manrope: `crates/hocus-shell/fonts`) |

The binary keeps its symbols, for backtraces.

## .deb

On Debian or Ubuntu, with `dpkg-dev` and the build dependencies of the CI:

```sh
cargo build --release --locked -p hocus-compositor
packaging/deb/build.sh                  # target/packages/hocuspanda_<version>-1_<arch>.deb
sudo apt install ./target/packages/hocuspanda_<version>-1_<arch>.deb
```

Library dependencies are the ones `dpkg-shlibdeps` finds on the build
system, so a package built on a release installs on that release and later
ones. EGL is opened at run time and is listed by hand.

GDM reads the sessions when its login screen starts: after installing,
log out (or restart GDM) and the gear under the password field lists
"Hocuspanda". GDM shows the gear only when more than one session is
installed.

To check the package against GDM in the Debian VM, logging in by the
keyboard as a person would:

```sh
scripts/vm/debian.sh up
scripts/vm/display-manager.sh target/packages/hocuspanda_<version>-1_<arch>.deb chooser.png session.png
scripts/vm/debian.sh down
```

## When the session does not start

The display manager comes back. Why is in the journal:

```sh
journalctl --user -t hocuspanda -b      # this boot's sessions; -b -1 for the previous boot
```

## Without a package

[`install.sh`](install.sh) installs the same files from a release build,
into `/usr/local` by default:

```sh
sudo packaging/install.sh /
```
