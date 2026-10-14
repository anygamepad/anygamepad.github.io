# SDL GameControllerDB

An unmodified copy of `gamecontrollerdb.txt` and `LICENSE` from
[SDL_GameControllerDB](https://github.com/mdqinc/SDL_GameControllerDB) at
commit `c1d5289a1f713b30a2c121e9fe6529d39360be0f` (2026-10-02): 2,279
mappings for Windows, macOS, Linux, Android and iOS.

License: zlib, Copyright (C) 1997-2025 Sam Lantinga (`LICENSE`). The notice
travels with every profile converted from it (`attribution`).

The importer (`anygamepad_db::sdl`, roadmap D2) converts each entry into a
profile with `source = "sdl-gamecontrollerdb"`, `verified = false` and
`indices = "sdl"`: its indices are SDL's joystick indices on the SDL
backend the entry's GUID names, so only providers that reproduce SDL's
order can apply them (`docs/research/02-sdl-gamecontrollerdb.md`).

To update: copy both files from a new commit, update the commit here and in
`VENDORED_ATTRIBUTION` (`crates/anygamepad-db/src/sdl.rs`), and run
`cargo test -p anygamepad-db --all-features`. The tests import every entry
and pin the count per platform; fields the importer does not know fail them.

Fixes found through anygamepad's own data go into anygamepad's profiles.
SDL_GameControllerDB asks for no AI-generated content in its issues and
pull requests, so contributions back must be written by people.
