# Fixtures

Data the scenarios use. `applications/` holds desktop entries, read by the
launcher when a scenario sets `data_dirs = ["../fixtures"]`: a few entries
named like real applications (their command does nothing), one hidden one,
and one that writes the environment it was started with into the
compositor's runtime directory.

`wallpapers/` holds two small pictures for the wallpaper scenarios: one
mostly orange (with a blue corner), one blue.
