# Packs release binaries already built: packaging/rpm/build.sh gives the
# checkout (hp_top), the target directory (hp_target) and the version.
# Linked libraries become dependencies through rpm's own scan of the
# binaries.

# No debug package and no stripping: the binaries carry their symbols, which
# panics and the watchdog's backtraces need to name functions.
%global debug_package %{nil}
%global __strip /bin/true

Name:           hocuspanda
Version:        %{hp_version}
Release:        1%{?dist}
Summary:        Wayland desktop: compositor and shell in one process
License:        MIT
URL:            https://github.com/mattboll/hocus-panda

# Opened at run time, which rpm's scan cannot see.
Requires:       libglvnd-egl
Requires:       libglvnd-gles
Requires:       fontconfig
Recommends:     mesa-libEGL
Recommends:     mesa-dri-drivers
Recommends:     foot
Recommends:     xorg-x11-server-Xwayland
Recommends:     google-noto-color-emoji-fonts

%description
Hocuspanda is a Wayland compositor and its whole shell (widget column,
launcher, notifications, quick settings, lock screen) in a single Rust
process. Display managers offer it as the "Hocuspanda" session.

This package holds the desktop (hocuspanda), its command-line tool (hocus)
and its settings app (hocus-settings).

%install
PREFIX=%{_prefix} SYSCONFDIR=%{_sysconfdir} CARGO_TARGET_DIR=%{hp_target} \
    %{hp_top}/packaging/install.sh --pam redhat %{buildroot}
install -D -m 644 %{hp_top}/LICENSE %{buildroot}%{_datadir}/licenses/%{name}/LICENSE

%files
%license %{_datadir}/licenses/%{name}/LICENSE
%{_bindir}/hocuspanda
%{_bindir}/hocus
%{_bindir}/hocus-settings
%{_datadir}/wayland-sessions/hocuspanda.desktop
%config(noreplace) %{_sysconfdir}/pam.d/hocuspanda
