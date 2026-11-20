# Architecture

How OP2Imager (and its second program, OP2Timelapse) is put together, for contributors and agents new to the code. It
follows the template at architecture.md. Keep it current when modules or data flow change.

## 1. Project structure

One crate, about 1,500 lines of Rust, with two programs. The drawing itself, running a mission's setup and reading the
game's files are op2dll's library (OP2MissionReader) and op2-rs's crates; this crate decides what to draw, at what
size, and writes the files.

```
op2imager/
├── Cargo.toml            two [[bin]]s; op2dll by path (no `app` feature), op2-settings pinned to op2dll's op2-rs commit
├── rust-toolchain.toml   Rust 1.99, as op2dll and op2-rs
├── README.md             usage, options, sizes, building
├── ARCHITECTURE.md       this file
├── CHANGELOG.md
├── LICENSE               GPL-3.0
└── src/
    ├── main.rs           OP2Imager: command line, the list of jobs, drawing each job, sizing, saving, --stats,
    │                     --compare
    ├── png.rs            a PNG encoder (8-bit RGB) that encodes one fixed way, the same as OP2TestHarness's
    ├── timelapse.rs      OP2Timelapse: an animated GIF of a run's whole-map images, each frame labelled with its tick
    └── gif.rs            the GIF writer: one shared median-cut palette, LZW, frames storing only changed pixels
```

## 2. High-level system diagram

```
   inputs: FILE | FOLDER (-r) ...                  Outpost 2 install (GOG 1.2 + OPU 1.4.1)
           │                                        tilesets, maps (loose or .vol), unit art, Outpost2.exe
           ▼                                                     │
   main::jobs ──► Job { Source, sub }                            ▼
     MapFile / MapNamed ── op2-formats Map ──────────┐     op2-formats GameFiles (main::Game, art loaded once)
     Dll ── op2dll inspect + setup::run (x86 sandbox)┤                │
     State ── op2dll setup::from_state ──────────────┤                │
                                                     ▼                ▼
                         op2dll export::draw (scene plan, sprites)   or   op2dll exact::compose (a state with views)
                                                     │                                 │
                                         main::sized (--size / --scale)      Canvas::shrink (the harness's average)
                                                     └───────────────┬─────────────────┘
                                                                     ▼
                                    png::encode (RGB, harness encoding)  |  op2dll export::save (JPEG, BMP, --rgba PNG)
                                                                     ▼
                                              DESTINATION/SUB/NAME.FMT   (+ NAME.json with --stats)

   OP2Timelapse:  map_<tick>.png ... ──► image (decode) ──► gif::write (palette, label bars, changed pixels) ──► .gif
   --compare A B [DIFF]:  two images ──► pixel by pixel ──► "identical" or where and how much they differ
```

Data flows one way: inputs become jobs, each job one image (and, with `--stats`, a JSON file beside it). Nothing is
written to the game folder.

## 3. Core components

**The job list (`main.rs`: `parse_args`, `jobs`, `folder_jobs`, `Job`, `Source`).**

- Options follow OP2MapImager's (`-s`/`--scale`, `-i`/`--image-format`, `-d`/`--destination`, `-o`/`--overwrite`,
  `-r`/`--recursive`, `-q`/`--quiet`, in any case), plus `--size`, `--rgba`, `--game-dir`, `--difficulty`,
  `--players`, `--no-objects` and `--stats`.
- Each input becomes `Job`s: a `.map` on disk (`MapFile`), a map name looked up in the game folder and its `.vol`
  archives (`MapNamed`), a DLL (`Dll`, with `listed` set when it came from a folder, so a DLL that isn't a mission is
  skipped quietly), or an OP2TestHarness state (`State`, `state_<tick>.json`). A folder gives its maps and DLLs sorted
  by name; with `-r` its subfolders follow, and `Job::sub` remembers the subfolder so the image lands in the same
  subfolder of the destination.
- `Job::image_path` names the image after its file: `DESTINATION/SUB/NAME.FMT`, or `NAME_TYPE.FMT` when a `.map` and a
  `.dll` of one name would share it.

**Drawing a job (`main.rs`: `run_job`, `Game`, `beside`, `sized`).** An image that exists is skipped unless
`--overwrite`. Then by source:

- **A map:** read with op2-formats and drawn by `export::draw` without objects.
- **A mission DLL:** `inspect::inspect` decides whether it is a mission; `setup::run` runs its `InitProc` in the
  sandbox with the chosen difficulty and player count (every slot by default). Its map is looked for beside the DLL
  first (`beside`, any case: a mission's own folder, as the game finds it), then in the game folder. The setup's tile
  edits are applied (`export::apply_graphics`) and `export::draw` draws the map with every object `scene::plan`
  places (`--no-objects` keeps the walls and tubes, drops the rest).
- **A harness state:** `setup::from_state` reads its tiles and units. If the state lists what the game drew (`views`
  and `viewSize`) and the game's art is there, `exact::compose` redraws it the way the game's detail view drew it, and
  `Canvas::shrink` averages it to the scale as the harness does: the result has the same pixels as the harness's
  `map_<tick>.png`. Otherwise (an older state, `--no-objects`, no art) it is drawn from its objects like a setup.
- `Game` holds the game files and loads the unit art the first time a job needs it (a warning, and dots, if it can't).
- `sized` brings an image to the size asked for: `--size` on the longer side (`export::fit`, Lanczos), else
  `--scale` pixels per tile (default 4) when that is smaller than drawn. Exact state images are already at their size.

**Saving (`main.rs`, `png.rs`).** A PNG is written by `png::encode` as 8-bit RGB (the alpha is always opaque, so it is
dropped). The encoder makes every choice one fixed way, written down in its module comment (Paeth on every row,
dynamic-Huffman blocks of 65536 symbols, greedy LZ77 with 32-deep hash chains, length-limited Huffman trees by a fixed
rule, a fixed run-length coding of the code lengths). OP2TestHarness's `png::Encode` follows the same rules, so equal
pixels give equal files and a state drawn exactly can be checked against the harness's image by file hash. JPEG, BMP
and `--rgba` PNGs go through op2dll's `export::save` (the `image` crate).

**`--stats` (`main.rs`: `setup_stats`).** Beside a mission's image, `NAME.json` (`format: op2imager-stats/1`): the
players, ore beacons by ore and yield (1-, 2-, 3-bar or random), magma vents, fumaroles, wrecks, vehicles,
structures, walls and tubes, and whether the setup ran to its end.

**`--compare A B [DIFF]` (`main.rs`: `compare`).** Handled before the other options: decodes two images to RGB and
compares them pixel by pixel. Prints `identical` (exit 0), or how many pixels differ, by how many levels at most and
within which rectangle (exit 1); DIFF is A with each differing pixel red (more than 8 levels off) or yellow.

**OP2Timelapse (`timelapse.rs`, `gif.rs`).** Takes PNG files or a folder (its `map_<tick>.png` in tick order, else its
`.png` files by name), all of one size, and writes an animated GIF. Each frame gets a label bar above it
(`TITLE  TICK N`, from `--title` and the file's name; `--no-label` drops it); `--delay` and `--last` set the frame
times. `gif.rs` builds one palette for every frame: 253 colours by median cut over 15-bit colours (the game draws in
15-bit), each colour weighted by the square root of its pixel count so rare, saturated colours keep entries of their
own, plus black and white for the labels and a transparent index. Each frame after the first stores only the pixels
that changed (the rest transparent), and the frames are LZW-coded.

## 4. Data stores

There is no database and nothing is cached on disk. The programs read:

- **The game folder:** `--game-dir`, else `$OP2_DIR`, else the folder op2-rs saved (op2-settings), else the usual
  install places. Only read.
- **The inputs:** maps, mission DLLs (read whole, mapped into the sandbox only for the setup), harness states and
  PNGs.

They write only the images (and `--stats` JSON files) under the destination, and the GIF OP2Timelapse is given.

## 5. External integrations

- **op2dll's library** (OP2MissionReader on GitHub at a pinned commit, crate `op2dll`, `default-features = false`):
  `inspect`, `setup` (a mission's setup in the sandbox, a harness state read), `scene`, `sprites`, `export` (drawing,
  resizing, saving) and `exact` (drawing a state as the game drew it). Fixes to drawing belong there, so op2dll's window
  and `render` get them too.
- **op2-rs** (gitlab.com/bufke/op2-rs, GPL-3.0-or-later), through op2dll: op2-formats (maps, tilesets, art, `.vol`
  archives), op2-x86 and op2-missionhost (running `InitProc`), op2-sim (the World behind the game API).
  `op2-settings` directly, pinned to the same commit as op2dll.
- **OP2TestHarness** (GitHub leviathan400/OP2TestHarness): its `state_<tick>.json` and `map_<tick>.png` (`--map-every`)
  are inputs. Its PNG encoder and `png.rs` must stay the same encoding; a change to one is a change to both.
- **OP2MapImager** (OutpostUniverse): the option names come from it, so its commands work here.
- **Other crates:** image (decoding PNGs, JPEG/BMP out through op2dll), serde_json (states, `--stats`), anyhow.

## 6. Deployment and infrastructure

Desktop command-line programs with no services. Built locally with `cargo build --release`, which fetches op2dll
(OP2MissionReader) and op2-rs's crates by git at pinned commits; the outputs are `target/release/OP2Imager.exe` and
`OP2Timelapse.exe`. The source is on GitHub (leviathan400/OP2Imager). CI (`.github/workflows/build.yml`)
builds both programs for Windows x64 and Linux x64 (Ubuntu 22.04, for older glibc) on every push, with clippy and the
tests, and keeps the builds as run artifacts; a `v*` tag attaches them, zipped, to that tag's release. Tried by hand on
Windows only; nothing used is Windows-only.

## 7. Security considerations

A mission DLL is untrusted code and never runs natively: its setup runs in op2-x86's interpreter through op2dll, with
only op2-missionhost's game API as a way out, and an instruction budget. Harness states and PNGs are parsed as data.
The only files written are the images, JSON files and GIFs under the paths given.

## 8. Development and testing

- Rust 1.99 (`rust-toolchain.toml`). Dependencies are optimized in dev builds too, because the sandbox is slow
  unoptimized.
- Keep `cargo clippy` clean.
- Unit tests: `png.rs` encodes images and decodes them back with the `image` crate (`round_trip`); `gif.rs` checks its
  LZW coder (`lzw_round_trip`) and that rare colours keep palette entries (`rare_colours_kept`).
- Checks by hand so far:
  - the game folder and OPU (`-r`) drawn whole: every map and mission DLL, none failing;
  - harness states drawn exactly against the harness's own images: 248 of 248 identical (`--compare`), and the same
    file hashes once both used the shared PNG encoding;
  - images looked at against the game.
- Game data and images are never committed.

## 9. Future considerations

- Saved games (`.op2`) aren't supported: drawing one would need the save format read into a `Setup`.
- An exact drawing of a mission's start (as `exact.rs` does for states) would need the game's view of it, which only
  a run in the game (OP2TestHarness) gives.
- The encoder in `png.rs` and the harness's must change together; a shared test vector (one image, one expected hash)
  in both repositories would catch them drifting apart.

## 10. Project identification

- **Name:** OP2Imager (crate `op2imager`, programs `OP2Imager` and `OP2Timelapse`), version 0.4.0.
- **Repository:** https://github.com/leviathan400/OP2Imager
- **Contact:** Leviathan (leviathan400).
- **Last updated:** 2026-11-20.

## 11. Glossary

- **Mission DLL:** an Outpost 2 level: a 32-bit Windows DLL exporting `InitProc`, `AIProc` and a descriptor.
- **Setup:** what a mission's `InitProc` places: units, structures, beacons, walls and tubes, tile edits.
- **Harness state:** OP2TestHarness's `state_<tick>.json`: the map's tiles, every unit and, by default, the views the
  game drew at one tick of a run.
- **Whole-map image:** OP2TestHarness's `map_<tick>.png`, the game's own drawing of the whole map at a tick.
- **Exact drawing:** a state redrawn the way the game's detail view draws it (15-bit colour, sprite layers, lines), so
  the pixels match the harness's whole-map image.
- **Scale:** pixels per tile; 32 is the game's own size.
- **File tile:** a tile in the map file's coordinates (the game's internal x is this plus the map's padding).
