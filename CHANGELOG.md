# Changelog

Notable changes to OP2Imager. The format follows [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

## [0.4.0] - 2026-11-20

### Added
- Icons and version details in the Windows exes: OP2Imager the starship (`assets/Tech_Starship.ico`), OP2Timelapse
  the space icon (`assets/Tech_Space.ico`); Properties > Details shows OP2Imager, Outpost Universe, version 0.4.0.
- Saved games (`.op2`): drawn as the game was when it was saved, the map with its tiles then and every unit, structure
  and beacon (a weapon's fire left out), from op2-formats 0.6's `saved_game`.
  A name alone is looked up in `OPU\saves` and then the game folder; a folder takes its `.op2` files and skips the
  ones that aren't games (`SGAME10.OP2`, the slot index). Drawn the same, pixel for pixel, as the harness's state of
  the same tick drawn from its objects, for four saves.

### Changed
- Tells op2dll's library the game folder (`setup::use_game_dir`), so a mission's setup runs with op2-rs's
  simulation: structures on dozed ground with their tubes, beacons with their art.
- One op2-formats, 0.6 (crates.io), as op2dll now uses: maps, art and saved games all come from it.
- `OP2Timelapse`, a second program: an animated GIF of OP2TestHarness's whole-map images (a folder's
  `map_<tick>.png` in tick order, or any PNGs of one size), each frame labelled with its tick (`--title`, `--no-label`,
  `--delay`, `--last`). One median-cut palette from the frames' 15-bit colours, with rare colours weighted up so team
  colours and lava keep theirs; frames after the first store only changed pixels (`gif.rs`, with tests of its LZW and
  palette).
- Game states: a `state_<tick>.json` from OP2TestHarness's whole-map images (`--map-every`) is drawn as that moment of
  the game, its tiles and objects as they were, with no game running (op2dll's `setup::from_state`). Folders take
  `state_*.json` files too. A state that lists what the game drew (OP2TestHarness's `views`) is drawn pixel for pixel
  as the game drew it (op2dll's `exact`): checked identical to the harness's own images, 248 of 248 over four whole
  campaign missions at every difficulty (8 px a tile; 32 px spot-checked).
- PNGs are 8-bit RGB, written by OP2Imager's own encoder (`png.rs`), the same as OP2TestHarness's for its whole-map
  images: the same pixels give the same file, so a state drawn exactly has the same SHA-256 as the harness's image
  (e05 at ticks 5 and 1000, 32 px a tile). About 40% of the size of the RGBA files before. `--rgba` writes those
  (alpha always opaque) instead.
- `--compare A B [DIFF]`: whether two images hold the same pixels (exit 0 if so), how many differ and where, with a
  picture of them.
- OP2Imager: images of `.map` files and mission `.dll` files (one file, several, or whole folders, `-r` for their
  subfolders) as PNG, JPEG or BMP. A mission is drawn as the game starts it, with every unit, structure, wall, tube and
  beacon its setup places (a multiplayer mission with all its player slots). OP2MapImager's options: `-s`/`--scale`
  (1-32 px per tile, default 4), `-i`/`--image-format`, `-d`/`--destination` (default `MapRenders`), `-o`/`--overwrite`,
  `-q`/`--quiet`, in any case; and `--size`, `--game-dir`, `--difficulty`, `--players`, `--no-objects`.
- With `-r` a subfolder's images go in the same subfolder of the destination, so files of one name in different
  folders each get one; a `.map` and `.dll` of one name in a folder get `NAME_dll.png` for the second.
- `--stats`: a mission's `NAME.json` beside its image, counting what its setup placed: ore beacons (common/rare by
  yield), magma vents, fumaroles, wrecks, units, structures, walls and tubes (`op2imager-stats/1`).
- A mission's map is taken from beside its DLL first (its own folder, as the game finds it), then from the game
  folder, so a mission whose map isn't installed can still be drawn (the OP2 Map Database's render server needs this).
- Built on op2dll's library (OP2MissionReader, without its window) and op2-rs 982336cf (v1.0.2).
