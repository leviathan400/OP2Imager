# OP2Imager

Makes images of Outpost 2 maps (`.map`) and mission DLLs (`.dll`), one file or whole folders at a time, as PNG, JPEG
or BMP.

- A **map** is drawn as its file has it: the terrain, cliffs, lava and so on.
- A **mission DLL** is drawn as the game starts it: its map with every unit, structure, wall, tube and mining beacon
  its setup places, in the players' colours. A multiplayer mission gets every player slot its map has, so every base
  is on it. Campaign missions use normal difficulty unless you choose another.
- A **game state** (`state_<tick>.json`, which OP2TestHarness writes beside its whole-map images with `--map-every`)
  is drawn as that moment of a running game: the map with the game's tiles then (dozed ground, tubes, walls, rubble)
  and every unit, structure and beacon where it stood, facing its way, in full daylight. It needs no game running; the
  map is the state's `map`, found beside the state file or in the game folder. A state that lists what the game drew
  (its `views`, which OP2TestHarness writes by default) is drawn exactly as the game drew it: the same pixels as the
  harness's own `map_<tick>.png` at the same scale (any `-s` that is a power of two; another draws 32 px a tile), cut
  to what the game showed. It needs the game's art. An older state without them, or one drawn with `--no-objects`,
  is drawn from its objects: units whole (damage isn't shown), facing kept to eight directions.
- A **saved game** (`.op2`, the game's `SGAME0.OP2` to `SGAME9.OP2`) is drawn as the game was when it was saved: the
  map with its tiles then and every unit, structure and beacon, drawn from its objects as a state is (whole, facing
  kept to eight directions, in full daylight). The save holds its own map, so it needs only the game's tilesets and
  art. A name alone is looked up in the game folder's `OPU\saves`, where OPU keeps them (`OP2Imager SGAME0.OP2`);
  `SGAME10.OP2` is the save dialog's index of the slots, not a game, and a folder skips it. Saves of the stock game
  and of OPU 1.4.x are both read. A save drawn this way and the harness's state of the same tick drawn from its
  objects give the same pixels (four saves at tick 3000: a campaign mission, a colony game, games of four and six
  players).
- **`--compare A B [DIFF]`** says whether two images hold the same pixels, whatever their file format: `identical`
  (exit 0), or how many pixels differ, by how much and where (exit 1), with DIFF marking them. For checking a drawn
  state against the game's image without looking at either.
- **PNGs are 8-bit RGB, encoded the way OP2TestHarness encodes its whole-map images** (src/png.rs, every choice fixed
  and written down there): the same pixels always give the same file, so a state drawn exactly has the same file hash
  as the harness's `map_<tick>.png`. `--rgba` writes a PNG with an (always opaque) alpha channel through the `image`
  crate instead.

It does what [OP2MapImager](https://github.com/OutpostUniverse/OP2MapImager) does for maps, with the same options, and
adds mission DLLs. Saved games (`.op2`) are drawn with their units, where OP2MapImager draws only their terrain.

It doesn't contain any of the game's files: the tilesets, unit art and a mission's map come from your Outpost 2
install (GOG 1.2 with OPU 1.4.1). A mission's setup (`InitProc`) runs in [op2-rs](https://gitlab.com/bufke/op2-rs)'s
x86 sandbox, not natively, through op2dll's library ([OP2MissionReader](https://github.com/leviathan400/OP2MissionReader)).

## Using it

```
OP2Imager [options] FILE|FOLDER ...
```

| Option | |
|---|---|
| `-s`, `--scale N` | pixels per tile: 1 (one pixel a tile) to 32 (the game's own size); default 4 |
| `--size PX` | instead of `--scale`: the image's longer side is at most PX pixels |
| `-i`, `--image-format FMT` | `png`, `jpg` or `bmp`; default `png` |
| `--rgba` | a PNG with an alpha channel (always opaque); by default PNGs are RGB in the harness's own encoding |
| `-d`, `--destination DIR` | where the images go; default `MapRenders` |
| `-o`, `--overwrite` | replace images already there (otherwise they're skipped) |
| `-r`, `--recursive` | a folder's subfolders too, their images in the same subfolders of the destination |
| `-q`, `--quiet` | print only errors |
| `--game-dir DIR` | the Outpost 2 folder; default `$OP2_DIR`, else the one op2-rs saved, else the usual install places |
| `--difficulty 0-2` | a mission's difficulty: 0 easy, 1 normal (default), 2 hard |
| `--players N` | a multiplayer mission's player count, 1-6; default every slot the map has |
| `--no-objects` | a mission's map with its walls and tubes but without units, structures and beacons |
| `--stats` | beside a mission's image, `NAME.json`: the ore beacons its setup placed (common and rare, by yield: 1-, 2-, 3-bar or random), magma vents, fumaroles, wrecks, units, structures, walls and tubes, players |

Options can be written in any case (`-S 8`, `--ImageFormat BMP`), as OP2MapImager takes them.

A `FILE` is a `.map` or `.dll`; a name alone that isn't in the current folder is looked up in the game folder (maps
in its `.vol` archives too). A `FOLDER` gives every `.map` and `.dll` in it: DLLs that aren't missions (`SDL2.dll`,
`op2ext.dll` and the like) and `wellpallet.map` are skipped. Each image is `DESTINATION/NAME.FMT` after its file's name;
with `-r`, a file in a subfolder has its image in the same subfolder of the destination, so files of one name in
different folders (OPU's backups of the campaign maps, map packs' own `op2extra.dll`) each get theirs. When a `.map`
and a `.dll` of one name would share an image, the second gets its type added (`NAME_dll.png`).

```
OP2Imager eden01.map e05.dll
OP2Imager -s 16 -o -i jpg ml4_21.dll
OP2Imager --size 1024 -i jpg -d web "C:\Games\Outpost 2"
OP2Imager -r -d opu "C:\Games\Outpost 2\OPU"
```

Sizes: at 32 px per tile a 128x128 map is 4096 px square, about 5-16 MB as a PNG (15-45 MB with `--rgba`); at the default 4 it's 512 px. For a
web page, `--size 1024 -i jpg` gives 100-300 KB a map. A map is drawn at up to 32 px per tile (16 for maps wider than
256 tiles) and then shrunk with a Lanczos filter, so small units and tile edges stay readable.

## Timelapses: OP2Timelapse

`OP2Timelapse` (a second program, built with OP2Imager) makes an animated GIF of a game's whole-map images:
OP2TestHarness's `map_<tick>.png`, written every N ticks with `--map-every N`, or any PNGs of one size.

```
OP2Timelapse --title E05 e05.gif runs\e05
```

A folder gives its `map_<tick>.png` in tick order (or its `.png` files by name). Each frame gets a bar above it with
its label, `TITLE  TICK 12000` (the tick from the file's name, else the file's name); `--no-label` leaves it out.
`--delay CS` sets each frame's time in hundredths of a second (default 40), `--last CS` the last frame's before it
loops (default four times the delay).

A GIF holds 256 colours, so the frames share one palette of 253 chosen by median cut over their 15-bit colours, rare
colours weighted up so team colours, lights and lava keep their own (src/gif.rs). It prints how close the palette came:
e05's 41 frames at 8 px a tile land 3.8 levels off on average, 192 of 21.3 million pixels more than 24. Each frame
after the first stores only the pixels that changed, so 41 frames of 512x1038 make a 418 KB GIF.

## Building

Needs Rust (the toolchain in `rust-toolchain.toml`). Cargo fetches op2dll's library from OP2MissionReader on GitHub
at a pinned commit, and op2-formats from crates.io. Saved games come from op2-formats 0.6's `saved_game` (crates.io).

```
cargo build --release
```

The programs are `target/release/OP2Imager.exe` and `OP2Timelapse.exe` (no `.exe` elsewhere).

## License

GPL-3.0-or-later (LICENSE), as op2-rs and op2dll, which it's built on.
