# Build and continuous integration

How the workspace is built the same way everywhere, and what CI checks
when. The branches and the lots are in [CLAUDE.md](../CLAUDE.md),
"Workflow".

## One toolchain, one lock file

- `rust-toolchain.toml` pins the exact Rust version; rustup installs it on
  the first `cargo` run, on a developer's machine and in CI alike.
  `rust-version` in `[workspace.package]` says the same to cargo, and every
  crate inherits it. Moving to a newer Rust is a commit of its own that
  changes both.
- Every build in CI runs with `--locked`: a `Cargo.lock` that does not
  match the manifests fails the build instead of being rewritten on the
  runner, so what CI tests is what the lock file says.
- Every dependency's version is declared once, in `[workspace.dependencies]`
  of the root `Cargo.toml`; the crates take them with `workspace = true`
  and add the features they need. A new dependency goes there first.
- `iced` and `iced_tiny_skia` are held at exact versions: the second is
  built from `vendor/` with a patch made on that version
  ([vendor/README.md](../vendor/README.md)).

## What runs when

| Job | Pull request | Push to `main`, `sprint/**` | Weekly |
|---|---|---|---|
| `fmt`, `clippy`, `licenses` (cargo-deny) | yes | yes | |
| `unused-dependencies` (cargo-machete) | yes | yes | |
| `vendor` (`scripts/vendor.sh --check`) | yes | yes | |
| `pr-tests`: the tests of what changed | yes | | |
| `build` and `linux (N/4)`: the whole suite | when asked (below) | yes | |
| `package`: the release build, the .deb and what it holds | | yes | |
| `smithay-debug-assertions` (`weekly.yml`) | | | yes |

The Rust caches are saved by `main` and by the sprint branches, where the
lots land; a pull request starts from its base branch's.

### Why a pull request runs only the tests of what it changes

A pull request runs the test files it changes and the unit tests of the
crates it changes (`scripts/ci/pr-tests.sh`), not the whole suite. This is
on purpose:

- The whole suite, built and run for every pull request, took 15 minutes
  and more (53 minutes when it ran in one job, #650), and a test failing
  by chance anywhere held every pull request, whatever it changed.
- Work reaches `main` through the sprint branches: an agent's lot is
  merged into `sprint/<N>` without a pull request, and **every push to a
  sprint branch runs the whole suite**, as every push to `main` does. The
  sprint's one pull request to `main` comes from a branch whose head has
  already passed everything. The whole suite is therefore the gate of
  what is merged; the pull request's selection is a fast first answer,
  not the only one.
- When the selection would not be enough, the pull request runs
  everything, as `main` does (built once, in four jobs): when it changes a
  test harness (`tests/common`, `tests/levels`, `tests/fakes`),
  `Cargo.lock`, the workspace's `Cargo.toml`, `rust-toolchain.toml` or
  nextest's configuration, or carries the label `full-tests`. A line
  `Tests: NAME, NAME` in its description adds test files.

A fix outside a sprint (one branch, one pull request to `main`) is the one
case where the whole suite first runs after the merge: give it the
`full-tests` label when it touches more than its own tests show.

### The package

`package` builds what the maintainer installs: the release build of the
five programs with `--locked`, the .deb (`packaging/deb/build.sh`), and
`packaging/deb/check.sh`, which fails when the package holds other files
than [packaging/README.md](../packaging/README.md) lists. The .deb is kept
as the run's artifact for a week.

### smithay's debug assertions

`Cargo.toml` builds smithay without its debug assertions, because a client
can trip some of them and end a debug session. Once a week, on `main`,
`weekly.yml` runs the whole suite with them on (through cargo's
configuration file: a profile's per-package settings cannot be set by an
environment variable): what they say of the compositor's own use of
smithay is read then, and a failure there is an issue to open, not a
broken build.

### Ignored advisories

Each advisory ignored in `deny.toml` has an owner, the day it was ignored
and a day to look at it again. On that day the line goes, or the date is
pushed with a reason.
