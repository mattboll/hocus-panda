# Packaging

What a package of Hocuspanda holds, and how each one is built. The
[Packages workflow](../.github/workflows/packages.yml) builds the .deb, the
.rpm and, from the FreeBSD port, a FreeBSD package, on `main`, on tags and
on pull requests that change this directory, and keeps them as the run's
artifacts.

## What is installed

| File | Where (Linux packages) | Why |
|---|---|---|
| `hocuspanda` | `/usr/bin` | The desktop |
| `hocus` | `/usr/bin` | Its command-line tool (`hocus msg`) |
| `hocus-settings` | `/usr/bin` | Its settings app, which the shell starts |
| [`hocuspanda.desktop`](hocuspanda.desktop) | `/usr/share/wayland-sessions` | Display managers offer the "Hocuspanda" session: `hocuspanda --backend tty`, the session itself on the virtual terminal they give it |
| [`pam/`](pam/) | `/etc/pam.d/hocuspanda` | The lock screen's password check (without it, hocuspanda uses the `login` service); one per distribution family, as their stacks differ |
| `LICENSE` | `/usr/share/doc/hocuspanda/copyright` (.deb), `/usr/share/licenses/hocuspanda` (.rpm) | MIT |

The test tools of `hocus-test` (`hocus-scenario`, `hocus-test-client`) are
not packaged.

The binaries keep their symbols: panics and the watchdog's backtraces name
functions with them.

## .deb

On Debian or Ubuntu, with `dpkg-dev` and the build dependencies of
[the workflow](../docs/workflow.md#tools):

```sh
cargo build --release --locked -p hocuspanda -p hocus -p hocus-settings
packaging/deb/build.sh                  # target/packages/hocuspanda_<version>-1_<arch>.deb
sudo apt install ./target/packages/hocuspanda_*.deb
```

Library dependencies are the ones `dpkg-shlibdeps` finds on the build
system, so a package built on a release installs on that release and later
ones: CI builds on Ubuntu 24.04, for Debian 13 and Ubuntu 24.04 onwards.
EGL, GLES and fontconfig are opened at run time and are listed by hand.

To check the package against a display manager in the Debian VM (greetd
and its text greeter, logging in by the keyboard as a person would):

```sh
scripts/vm/debian.sh up
scripts/vm/display-manager.sh target/packages/hocuspanda_*.deb shot.png
scripts/vm/debian.sh down
```

GDM starts it too. When Hocuspanda is the only session installed, GDM
shows no session chooser and falls back to its own login session; once the
user's session is Hocuspanda (chosen in GDM's chooser when another desktop
is installed, or set through AccountsService), it starts.

## .rpm

On Fedora, with `rpm-build` and the build dependencies listed in the
workflow's `rpm` job:

```sh
cargo build --release --locked -p hocuspanda -p hocus -p hocus-settings
packaging/rpm/build.sh                  # target/packages/hocuspanda-<version>-1.<dist>.<arch>.rpm
sudo dnf install ./target/packages/hocuspanda-*.rpm
```

[`hocuspanda.spec`](rpm/hocuspanda.spec) packs the binaries already built;
rpm finds the linked libraries itself.

## Without a package

[`install.sh`](install.sh) installs the same files from a release build,
into `/usr/local` by default:

```sh
sudo packaging/install.sh --pam debian /     # or --pam redhat, or no PAM service
```

## FreeBSD port

[`freebsd/x11-wm/hocuspanda`](freebsd/x11-wm/hocuspanda) is a port for the
ports tree: it builds the three programs with the ports' Cargo support and
installs them, the session file in `/usr/local/share/wayland-sessions` and
the PAM service as `/usr/local/etc/pam.d/hocuspanda` (from
[`pam/freebsd`](pam/freebsd)). `pkg-message` tells how to run seatd, which
gives the session its devices.

The port fetches a tagged release from GitHub (`v<DISTVERSION>`) and each
crate of `Cargo.lock`. When a release is tagged, in a ports tree:

```sh
cp -R packaging/freebsd/x11-wm/hocuspanda /usr/ports/x11-wm/
cd /usr/ports/x11-wm/hocuspanda
# DISTVERSION= the release, in the Makefile
make makesum                            # the release's tarball
make cargo-crates > Makefile.crates     # the crates of its Cargo.lock
make makesum                            # and theirs
make clean                              # extracted again, with the crates
make stage check-plist package
```

Before the first release, `HOCUSPANDA_COMMIT=<commit>` on each `make`
builds a commit instead, as the workflow's `freebsd` job does (with `make
install-missing-packages` first, to take the dependencies from packages).
