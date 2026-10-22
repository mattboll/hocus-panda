# Portal files

Screen sharing in browsers and video calls goes through
xdg-desktop-portal. In a Hocuspanda session, xdg-desktop-portal-wlr
streams the screen through PipeWire on the compositor's capture protocols,
and asks `hocus share-pick` what to share: the shell shows the screens and
the windows in its own look.

| File | Installed as |
|---|---|
| `hocuspanda-portals.conf` | `/usr/share/xdg-desktop-portal/hocuspanda-portals.conf` |
| `xdg-desktop-portal-wlr.conf` | `/etc/xdg/xdg-desktop-portal-wlr/Hocuspanda` |

The packages (#53) install them; by hand, copy them there, or put the second
in `~/.config/xdg-desktop-portal-wlr/Hocuspanda`.
