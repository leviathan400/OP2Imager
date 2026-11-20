//! OP2Imager: renders Outpost 2 maps (`.map`) and mission DLLs (`.dll`) to PNG, JPEG or BMP images.
//!
//! A map is drawn as its file has it. A mission DLL is drawn as the game starts it: its setup (`InitProc`) runs in
//! op2-rs's x86 sandbox through op2dll's library, and its map is drawn with every unit, structure, wall, tube and
//! mining beacon it placed (a multiplayer mission with every player slot it has). A saved game (`.op2`) is drawn as
//! the game was when it was saved. Its options follow OP2MapImager's (`-s`, `-i`, `-d`, `-o`, `-q`), so the same
//! commands work.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use image::RgbaImage;
use op2_formats::render::MapTilesets;
use op2_formats::{GameFiles, Map};
use op2dll::sprites::Art;
use op2dll::{exact, export, inspect, scene, setup};
use op2dll::setup::Kind;

mod png;

const USAGE: &str = "\
OP2Imager: images of Outpost 2 maps and mission DLLs

usage: OP2Imager [options] FILE|STATE|FOLDER ...

  FILE     a .map file, a mission .dll or a saved game (.op2); a name alone is also looked up in the game folder
           (maps in its .vol archives too, saved games in OPU\\saves)
  STATE    a game state OP2TestHarness wrote beside its whole-map images (state_<tick>.json): that moment of a game,
           its map found beside the file or in the game folder
  FOLDER   every .map, mission .dll, saved game and state_*.json in it (DLLs that aren't missions, files that
           aren't saved games such as SGAME10.OP2, and wellpallet.map, are skipped); with -r in the folders under it
           too (OPU's maps folder keeps each map in a folder of its own)

  -s, --scale N            pixels per tile, 1 (one pixel a tile) to 32 (the game's own size); default 4
      --size PX            instead of --scale: the image's longer side is at most PX pixels
  -i, --image-format FMT   png, jpg or bmp; default png
      --rgba               a PNG with an alpha channel (always opaque), as before; by default PNGs are RGB, encoded
                           the way OP2TestHarness encodes its whole-map images, so the same pixels give the same file
  -d, --destination DIR    where the images go; default MapRenders
  -o, --overwrite          replace images already there (otherwise they're skipped)
  -r, --recursive          a folder's subfolders too; their images go in the same subfolders of the destination
  -q, --quiet              print only errors
      --game-dir DIR       the Outpost 2 folder (tilesets, unit art, a mission's map); default $OP2_DIR, else the
                           one op2-rs saved, else the usual install places
      --difficulty 0-2     a mission's difficulty: 0 easy, 1 normal (default), 2 hard
      --players N          a multiplayer mission's player count, 1-6; default every slot the map has
      --no-objects         a mission's map with its walls and tubes but without units, structures and beacons
      --stats              beside a mission's image, NAME.json: the ore beacons (common/rare, by yield), magma vents,
                           fumaroles, wrecks, units and structures its setup placed
  -h, --help               this text
  -V, --version            the version
      --compare A B [DIFF] only compare two images' pixels: `identical` (exit 0) or how many differ (exit 1),
                           DIFF marking them

Each image is DESTINATION/NAME.FMT, after the file's name (e05.dll -> MapRenders/e05.png). When a .map and a .dll
of one name would share an image, the second gets its type added (NAME_dll.FMT).
Examples:
  OP2Imager eden01.map e05.dll
  OP2Imager -s 16 -o -i jpg ml4_21.dll
  OP2Imager --size 1024 -i jpg -d web \"C:\\Games\\Outpost 2\"";

/// The image formats, by the extension they're saved with.
const FORMATS: [&str; 3] = ["png", "jpg", "bmp"];

/// The command line.
struct Options {
    inputs: Vec<PathBuf>,
    scale: u32,
    size: Option<u32>,
    format: &'static str,
    destination: PathBuf,
    overwrite: bool,
    recursive: bool,
    quiet: bool,
    game_dir: Option<PathBuf>,
    difficulty: Option<i32>,
    players: Option<u8>,
    no_objects: bool,
    stats: bool,
    /// PNGs with alpha, through the `image` crate, instead of `png::encode`'s RGB.
    rgba: bool,
}

/// The parsed command line, or `None` when it asked for the help or the version (already printed).
fn parse_args() -> Result<Option<Options>> {
    let mut o = Options {
        inputs: Vec::new(),
        scale: 4,
        size: None,
        format: "png",
        destination: PathBuf::from("MapRenders"),
        overwrite: false,
        recursive: false,
        quiet: false,
        game_dir: None,
        difficulty: None,
        players: None,
        no_objects: false,
        stats: false,
        rgba: false,
    };
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        if !text.starts_with('-') || text == "-" {
            o.inputs.push(PathBuf::from(arg));
            continue;
        }
        // Options in any case, as OP2MapImager takes them (-S, --Scale).
        let flag = text.to_ascii_lowercase();
        let mut value = |what: &str| args.next().map(|v| v.to_string_lossy().into_owned()).with_context(|| format!("{flag} needs {what}"));
        match flag.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "-v" | "--version" => {
                println!("OP2Imager {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "-s" | "--scale" => o.scale = value("a number")?.parse().ok().filter(|n| (1..=32).contains(n)).context("--scale: 1 to 32 pixels per tile")?,
            "--size" => o.size = Some(value("a number")?.parse().ok().filter(|n| (16..=16_384).contains(n)).context("--size: 16 to 16384 pixels")?),
            "-i" | "--image-format" | "--imageformat" => {
                let f = value("png, jpg or bmp")?.to_ascii_lowercase();
                let f = if f == "jpeg" { "jpg".to_string() } else { f };
                o.format = FORMATS.into_iter().find(|&x| x == f).with_context(|| format!("--image-format: png, jpg or bmp, not {f}"))?;
            }
            "-d" | "--destination" | "--destinationdirectory" => o.destination = PathBuf::from(value("a folder")?),
            "-o" | "--overwrite" => o.overwrite = true,
            "-r" | "--recursive" => o.recursive = true,
            "-q" | "--quiet" => o.quiet = true,
            "--game-dir" => o.game_dir = Some(PathBuf::from(value("a folder")?)),
            "--difficulty" => o.difficulty = Some(value("0, 1 or 2")?.parse().ok().filter(|d| (0..=2).contains(d)).context("--difficulty: 0 easy, 1 normal, 2 hard")?),
            "--players" => o.players = Some(value("a number")?.parse().ok().filter(|n| (1..=6).contains(n)).context("--players: 1 to 6")?),
            "--no-objects" => o.no_objects = true,
            "--stats" => o.stats = true,
            "--rgba" => o.rgba = true,
            _ => bail!("unknown option {text}\n\n{USAGE}"),
        }
    }
    if o.inputs.is_empty() {
        bail!("no map, DLL or folder given\n\n{USAGE}");
    }
    Ok(Some(o))
}

/// One thing to draw.
struct Job {
    source: Source,
    /// With `-r`, the subfolder of the folder given that it was found in; its image goes in the same subfolder of the
    /// destination, so files of the same name in different folders (OPU's backups, map packs) each get one.
    sub: PathBuf,
}

/// Where a job's map comes from.
enum Source {
    /// A `.map` file on disk.
    MapFile(PathBuf),
    /// A map by name from the game folder (loose or in a `.vol` archive).
    MapNamed(String),
    /// A DLL; `listed` when it came from a folder, where one that isn't a mission is skipped quietly.
    Dll { path: PathBuf, listed: bool },
    /// A running game's state as OP2TestHarness writes it beside its whole-map images (`state_<tick>.json`).
    State(PathBuf),
    /// A saved game; `listed` when it came from a folder, where a file that isn't one (the dialog's slot index,
    /// `SGAME10.OP2`) is skipped quietly.
    Save { path: PathBuf, listed: bool },
}

impl Job {
    fn new(source: Source) -> Job {
        Job { source, sub: PathBuf::new() }
    }

    /// The file's name (`e05.dll`).
    fn name(&self) -> String {
        match &self.source {
            Source::MapFile(p) | Source::Dll { path: p, .. } | Source::State(p) | Source::Save { path: p, .. } => {
                p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            }
            Source::MapNamed(n) => n.clone(),
        }
    }

    /// The image's path: `DESTINATION/SUB/NAME.FMT`, or `NAME_TYPE.FMT` when a file of the same name and another type
    /// already took that this run (a `.map` and a `.dll` side by side).
    fn image_path(&self, o: &Options, taken: &HashSet<PathBuf>) -> PathBuf {
        let name = self.name();
        let file = Path::new(&name);
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = o.destination.join(&self.sub);
        let plain = dir.join(format!("{stem}.{}", o.format));
        if !taken.contains(&plain) {
            return plain;
        }
        let ext = file.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        dir.join(format!("{stem}_{ext}.{}", o.format))
    }
}

fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// A folder's maps and DLLs, sorted by name, then (with `recursive`) its subfolders' in turn; `sub` is `dir` within
/// the folder given.
fn folder_jobs(dir: &Path, sub: &Path, recursive: bool, out: &mut Vec<Job>) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?.flatten().map(|e| e.path()).collect();
    entries.sort_by_key(|p| p.file_name().map(|f| f.to_ascii_lowercase()));
    for p in entries.iter().filter(|p| p.is_file()) {
        // wellpallet.map holds the tilesets' palette for the map editor, not a map to draw.
        if has_ext(p, "map") && !p.file_name().is_some_and(|f| f.eq_ignore_ascii_case("wellpallet.map")) {
            out.push(Job { source: Source::MapFile(p.clone()), sub: sub.to_path_buf() });
        } else if has_ext(p, "dll") {
            out.push(Job { source: Source::Dll { path: p.clone(), listed: true }, sub: sub.to_path_buf() });
        } else if has_ext(p, "json") && p.file_name().is_some_and(|f| f.to_string_lossy().starts_with("state_")) {
            out.push(Job { source: Source::State(p.clone()), sub: sub.to_path_buf() });
        } else if has_ext(p, "op2") {
            out.push(Job { source: Source::Save { path: p.clone(), listed: true }, sub: sub.to_path_buf() });
        }
    }
    if recursive {
        for d in entries.iter().filter(|p| p.is_dir()) {
            folder_jobs(d, &sub.join(d.file_name().unwrap_or_default()), true, out)?;
        }
    }
    Ok(())
}

/// The jobs the inputs name, in the order given.
fn jobs(inputs: &[PathBuf], game_dir: Option<&Path>, recursive: bool) -> Result<Vec<Job>> {
    let mut out = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let before = out.len();
            folder_jobs(input, Path::new(""), recursive, &mut out)?;
            if out.len() == before {
                let hint = if recursive { "" } else { " (-r looks in its subfolders too)" };
                eprintln!("{}: no .map, .dll, .op2 or state_*.json files in it{hint}", input.display());
            }
        } else if input.is_file() {
            if has_ext(input, "map") {
                out.push(Job::new(Source::MapFile(input.clone())));
            } else if has_ext(input, "dll") {
                out.push(Job::new(Source::Dll { path: input.clone(), listed: false }));
            } else if has_ext(input, "json") {
                out.push(Job::new(Source::State(input.clone())));
            } else if has_ext(input, "op2") {
                out.push(Job::new(Source::Save { path: input.clone(), listed: false }));
            } else {
                bail!("{}: not a .map, .dll, .op2 or state .json file", input.display());
            }
        } else {
            // A name alone: from the game folder.
            let name = input.to_string_lossy().into_owned();
            let bare = input.components().count() == 1;
            match (bare, game_dir) {
                (true, Some(_)) if has_ext(input, "map") => out.push(Job::new(Source::MapNamed(name))),
                (true, Some(dir)) if has_ext(input, "dll") => {
                    let path = op2_formats::find_in_dir(dir, &name).map_err(|_| anyhow!("{name}: not found here or in {}", dir.display()))?;
                    out.push(Job::new(Source::Dll { path, listed: false }));
                }
                // OPU keeps the save slots in OPU\saves; the stock game beside Outpost2.exe.
                (true, Some(dir)) if has_ext(input, "op2") => {
                    let path = [dir.join("OPU").join("saves"), dir.to_path_buf()]
                        .iter()
                        .find_map(|d| op2_formats::find_in_dir(d, &name).ok())
                        .ok_or_else(|| anyhow!("{name}: not found here, in {}\\OPU\\saves or in {0}", dir.display()))?;
                    out.push(Job::new(Source::Save { path, listed: false }));
                }
                _ => bail!("{}: not found", input.display()),
            }
        }
    }
    Ok(out)
}

/// What a job came to.
enum Outcome {
    Saved { path: PathBuf, width: u32, height: u32, what: String },
    /// The image is there already and `--overwrite` wasn't given.
    Exists(PathBuf),
    /// A DLL from a folder that isn't a mission.
    NotAMission,
    /// An `.op2` from a folder that isn't a saved game.
    NotASave,
}

/// The game's files, and the unit art once a mission has needed it.
struct Game {
    files: GameFiles,
    art: Option<Option<Art>>,
}

impl Game {
    fn art(&mut self) -> Option<&Art> {
        let files = &self.files;
        self.art
            .get_or_insert_with(|| Art::load(files).map_err(|e| eprintln!("warning: no unit art, units are drawn as dots: {e}")).ok())
            .as_ref()
    }
}

/// Draws `job` and saves it to `out`.
fn run_job(job: &Job, out: &Path, game: &mut Game, game_dir: &Path, o: &Options) -> Result<Outcome> {
    if out.exists() && !o.overwrite {
        return Ok(Outcome::Exists(out.to_path_buf()));
    }
    let mut stats = None;
    let mut drawn_exact = false;
    let (map, img, what) = match &job.source {
        Source::MapFile(path) => {
            let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            let map = Map::parse(&data)?;
            let img = export::draw(&map, &MapTilesets::load(&game.files, &map), None, None, &scene::Layers::default());
            (map, img, "map".to_string())
        }
        Source::MapNamed(name) => {
            let map = game.files.read_map(name)?;
            let img = export::draw(&map, &MapTilesets::load(&game.files, &map), None, None, &scene::Layers::default());
            (map, img, "map".to_string())
        }
        Source::Dll { path, listed } => {
            let report = inspect::inspect(path, Some(game_dir))?;
            let Some(desc) = report.desc.as_ref() else {
                if *listed {
                    return Ok(Outcome::NotAMission);
                }
                bail!("not a mission: {}", report.not_a_mission.as_deref().unwrap_or("?"));
            };
            let mut start = setup::StartOptions::default_for(desc.mission_type, desc.num_players);
            start.difficulty = o.difficulty.unwrap_or(start.difficulty);
            start.players = o.players.or(start.players);
            let setup = setup::run(path, start)?;
            // The map beside the DLL first (a mission's own folder, as the game finds it), then the game's files.
            let beside = beside(path, &desc.map_name);
            let mut map = match &beside {
                Some(p) => Map::parse(&std::fs::read(p).with_context(|| format!("reading {}", p.display()))?)
                    .with_context(|| format!("its map {}", p.display()))?,
                None => game.files.read_map(&desc.map_name).with_context(|| format!("its map {}", desc.map_name))?,
            };
            if let Some(g) = &setup.graphics {
                export::apply_graphics(&mut map, g);
            }
            let tilesets = MapTilesets::load(&game.files, &map);
            let layers = scene::Layers { objects: !o.no_objects, ..scene::Layers::default() };
            let img = export::draw(&map, &tilesets, Some(&setup), game.art(), &layers);
            if o.stats {
                stats = Some(setup_stats(&setup, &desc.map_name));
            }
            (map, img, format!("{}, {}", desc.map_name, setup.summary()))
        }
        Source::Save { path, listed } => {
            let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            if !op2_formats::saved_game::SavedGame::is_saved_game(&data) {
                if *listed {
                    return Ok(Outcome::NotASave);
                }
                bail!("not a saved game (SGAME10.OP2 is the save dialog's slot index)");
            }
            let save = op2_formats::saved_game::SavedGame::parse(&data)?;
            let (map, state) = saved_state(&save)?;
            let tilesets = MapTilesets::load(&game.files, &map);
            let layers = scene::Layers { objects: !o.no_objects, ..scene::Layers::default() };
            let img = export::draw(&map, &tilesets, Some(&state.setup), game.art(), &layers);
            let named = if save.description.is_empty() { String::new() } else { format!(" \"{}\"", save.description) };
            (map, img, format!("{}{named} at tick {}, {}", save.mission, save.tick, state.setup.summary()))
        }
        Source::State(path) => {
            let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
            let state = setup::from_state(&text).with_context(|| path.display().to_string())?;
            // The map beside the state first (a mission's own map, which the game may have from a mod's archive), then
            // the game's files.
            let beside = beside(path, &state.map_name);
            let mut map = match &beside {
                Some(p) => Map::parse(&std::fs::read(p).with_context(|| format!("reading {}", p.display()))?)
                    .with_context(|| format!("its map {}", p.display()))?,
                None => game.files.read_map(&state.map_name).with_context(|| {
                    format!("its map {} (not beside the state, nor in the game folder)", state.map_name)
                })?,
            };
            if let Some(g) = &state.setup.graphics {
                export::apply_graphics(&mut map, g);
            }
            let tilesets = MapTilesets::load(&game.files, &map);
            // What the game drew for this image, when the state lists it (OP2TestHarness's `views` and `viewSize`):
            // drawn as the game draws it, the same pixels as the harness's image (op2dll's `exact`). It needs the
            // game's art; without it, with --no-objects or for an older state, it is drawn from the state's objects.
            let v: serde_json::Value = serde_json::from_str(&text)?;
            let views = exact::views_of(&v);
            if let (false, true, Some(art), false) = (views.is_empty(), v["viewSize"].is_array(), game.art(), o.no_objects) {
                // each player's colour, by player number, for tinting their sprites
                let colors: Vec<u8> = v["players"].as_array().map(|ps| ps.iter().map(|p| p["colour"].as_u64().unwrap_or(0) as u8).collect()).unwrap_or_default();
                // the animated tiles' frame counter, the tick in every state measured
                let counter = v["animCounter"].as_u64().unwrap_or(state.tick as u64) as u32;
                let terrain = exact::Canvas::terrain(&map, &tilesets, counter);
                let size = |i: usize| v["viewSize"][i].as_i64().unwrap_or(0) as i32;
                let canvas = exact::compose(&terrain, art.sprites(), &views, (size(0), size(1)),
                    v["padding"].as_i64().unwrap_or(0) as i32, &colors);
                let draws: usize = views.iter().map(|v| v.draws.len()).sum();
                // the area the game showed (it never shows the map's last row), in game pixels: the state's image
                // size (`shown`) at its scale (`px`)
                let f = 32 / v["px"].as_u64().unwrap_or(32).clamp(1, 32) as u32;
                let shown = |i: usize| v["shown"][i].as_u64().map(|n| n as u32 * f);
                let (w, h) = (shown(0).unwrap_or(canvas.width), shown(1).unwrap_or(canvas.height));
                // the harness's block average needs a whole number of pixels a block: another -s draws 32 px a tile
                let px = if o.scale.is_power_of_two() { o.scale } else { 32 };
                let (w, h, rgb) = canvas.shrink(w, h, px);
                let rgba = rgb.as_chunks::<3>().0.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect();
                drawn_exact = true;  // already at its size: `sized` would scale it again
                let img = RgbaImage::from_raw(w, h, rgba).context("an image of its size")?;
                (map, img, format!("{} at tick {}, {draws} draws as the game drew them", state.map_name, state.tick))
            } else {
                let layers = scene::Layers { objects: !o.no_objects, ..scene::Layers::default() };
                let img = export::draw(&map, &tilesets, Some(&state.setup), game.art(), &layers);
                (map, img, format!("{} at tick {}, {}", state.map_name, state.tick, state.setup.summary()))
            }
        }
    };
    let img = if drawn_exact { img } else { sized(img, &map, o) };
    let (width, height) = img.dimensions();
    let dir = out.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    if o.format == "png" && !o.rgba {
        // RGB in the harness's own encoding: the alpha (always opaque) is dropped
        let rgb: Vec<u8> = img.as_raw().as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        std::fs::write(out, png::encode(&rgb, width, height)).with_context(|| format!("writing {}", out.display()))?;
    } else {
        export::save(out, img)?;
    }
    if let Some(st) = stats {
        let p = out.with_extension("json");
        std::fs::write(&p, serde_json::to_string_pretty(&st)?).with_context(|| format!("writing {}", p.display()))?;
    }
    Ok(Outcome::Saved { path: out.to_path_buf(), width, height, what })
}

/// What a mission's setup placed, for `--stats`: ore beacons by ore and yield (1-, 2-, 3-bar, or "random" where the
/// mission let the game choose), magma vents, fumaroles, wrecks, units, structures, walls and tubes, the players.
fn setup_stats(setup: &setup::Setup, map_name: &str) -> serde_json::Value {
    use serde_json::json;
    let mut ore = [[0u32; 4]; 2]; // [common, rare][1-bar, 2-bar, 3-bar, random]
    let (mut vents, mut fumaroles, mut wrecks, mut vehicles, mut structures) = (0, 0, 0, 0, 0);
    for ob in &setup.objects {
        match ob.type_name.as_str() {
            "BEACON" => {
                let rare = usize::from(ob.extra.get("oreType") == Some(&1));
                let slot = match ob.extra.get("bars") { Some(&b @ 1..=3) => b as usize - 1, _ => 3 };
                ore[rare][slot] += 1;
            }
            "MAGMA_VENT" => vents += 1,
            "FUMAROLE" => fumaroles += 1,
            "WRECK_FLAG" => wrecks += 1,
            _ => match ob.kind {
                Kind::Vehicle => vehicles += 1,
                Kind::Structure => structures += 1,
                _ => {}
            },
        }
    }
    let beacons = |o: [u32; 4]| json!({"total": o.iter().sum::<u32>(), "1_bar": o[0], "2_bar": o[1], "3_bar": o[2], "random": o[3]});
    json!({
        "format": "op2imager-stats/1",
        "map": map_name,
        "players": setup.players.len(),
        "human_players": setup.players.iter().filter(|p| p.human).count(),
        "beacons": {"common": beacons(ore[0]), "rare": beacons(ore[1])},
        "magma_vents": vents,
        "fumaroles": fumaroles,
        "wrecks": wrecks,
        "vehicles": vehicles,
        "structures": structures,
        "walls_tubes": setup.walls.len(),
        "complete": setup.failed.is_none(),
        "stopped": setup.failed,
    })
}

/// `name` (a bare file name) in the same folder as `file`, in any case: a mission's map in its own folder.
fn beside(file: &Path, name: &str) -> Option<PathBuf> {
    if Path::new(name).components().count() != 1 {
        return None;
    }
    let dir = file.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(name))
    })
}

/// A saved game as a state to draw: its map as it was, its
/// live units but a weapon's fire, its players, at its tick.
fn saved_state(save: &op2_formats::saved_game::SavedGame) -> Result<(Map, setup::State)> {
    let map = save.map.clone();
    let pad = map.x_padding() * 32;
    let (w, h) = (map.width, map.height);
    let graphics = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| ((map.tile_bits(x, y) >> 5) & 0x7FF) as u16).collect();
    let players = save
        .players
        .iter()
        .map(|p| setup::StatePlayer { id: p.id as i32, eden: p.eden, human: p.human, colour: p.colour as i32 })
        .collect();
    let objects = save
        .units
        .iter()
        .filter(|u| !setup::is_weapon_fire(u.type_id as i32, u.flags))
        .map(|u| {
            let type_name = setup::type_name(u.type_id as i32);
            let beacon = (type_name == "BEACON").then(|| {
                let (ore, bar, _) = u.mining_beacon();
                (ore as i64, bar as i64)
            });
            let truck = (type_name == "BIG_TRUCK").then(|| {
                let (cargo, amount) = u.truck_cargo();
                (cargo as i64, amount as i64)
            });
            setup::StateObject {
                index: u.slot,
                type_name,
                owner: u.owner as i32,
                x: u.pixel_x - pad,
                y: u.pixel_y,
                rotation: u.rotation as i32,
                cargo: u.cargo as i32,
                truck,
                beacon,
            }
        })
        .collect();
    let parts = setup::StateParts {
        width: w as i64,
        height: h as i64,
        graphics,
        players,
        objects,
        map_name: String::new(),
        mission: save.mission.clone(),
        tick: save.tick as i64,
    };
    Ok((map, setup::from_parts(parts)))
}

/// The drawn map at the size asked for: `--size` on its longer side, else `--scale` pixels per tile (a map drawn at
/// less than that, one wider than 256 tiles, stays as drawn).
fn sized(img: RgbaImage, map: &Map, o: &Options) -> RgbaImage {
    match o.size {
        Some(side) => export::fit(img, side),
        None if o.scale < export::scale_for(map) => export::resize(img, map.width * o.scale, map.height * o.scale),
        None => img,
    }
}

/// `--compare A B [DIFF]`: whether two images hold the same pixels (their colours, whatever the file format), so a
/// drawn state can be checked against the game's own whole-map image without looking at either. Prints `identical`
/// (exit 0) or how many pixels differ, by how much and where (exit 1); DIFF, if given, is A with each differing pixel
/// red (more than 8 levels off) or yellow.
fn compare(args: &[String]) -> Result<ExitCode> {
    let [a, b, rest @ ..] = args else { bail!("usage: OP2Imager --compare A B [DIFF]") };
    let load = |p: &String| image::open(p).with_context(|| format!("reading {p}")).map(|i| i.into_rgb8());
    let (ia, ib) = (load(a)?, load(b)?);
    if ia.dimensions() != ib.dimensions() {
        println!("different sizes: {}x{} and {}x{}", ia.width(), ia.height(), ib.width(), ib.height());
        return Ok(ExitCode::from(1));
    }
    let mut diff = ia.clone();
    let (mut n, mut max) = (0u64, 0u8);
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, pa) in ia.enumerate_pixels() {
        let pb = ib.get_pixel(x, y);
        let d = (0..3).map(|c| pa[c].abs_diff(pb[c])).max().unwrap_or(0);
        if d == 0 {
            continue;
        }
        n += 1;
        max = max.max(d);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        diff.put_pixel(x, y, image::Rgb(if d > 8 { [255, 0, 0] } else { [255, 255, 0] }));
    }
    if n == 0 {
        println!("identical ({}x{})", ia.width(), ia.height());
        return Ok(ExitCode::SUCCESS);
    }
    let total = ia.width() as u64 * ia.height() as u64;
    println!("{n} of {total} pixels differ, by up to {max} levels, within ({x0},{y0})-({x1},{y1})");
    if let Some(out) = rest.first() {
        diff.save(out).with_context(|| format!("writing {out}"))?;
    }
    Ok(ExitCode::from(1))
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("OP2Imager: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    // --compare takes no other options, so it's handled before the usual ones
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--compare") {
        return compare(&args[1..]);
    }
    let Some(o) = parse_args()? else { return Ok(ExitCode::SUCCESS) };
    let game_dir = op2_formats::install::use_for_tool(o.game_dir.clone(), op2_settings::Settings::load().game_dir)
        .map_err(|e| anyhow!("{e}\nThe tilesets and unit art come from the game folder: give it with --game-dir"))?;
    // op2-rs's mission host reads the game folder from its own op2-formats: without it a setup has no simulation.
    setup::use_game_dir(&game_dir);
    let jobs = jobs(&o.inputs, Some(&game_dir), o.recursive)?;
    let mut game = Game { files: GameFiles::open(&game_dir).with_context(|| format!("game folder {}", game_dir.display()))?, art: None };
    let (mut saved, mut existed, mut not_missions, mut not_saves, mut failed) = (0, 0, 0, 0, Vec::new());
    // The images this run has claimed, so two files of one name never share one.
    let mut taken = HashSet::new();
    for job in &jobs {
        let name = job.name();
        let out = job.image_path(&o, &taken);
        taken.insert(out.clone());
        // A mission's own code runs in the x86 sandbox: a panic there costs that file, not the run.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(job, &out, &mut game, &game_dir, &o)))
            .unwrap_or_else(|_| Err(anyhow!("panicked")));
        match result {
            Ok(Outcome::Saved { path, width, height, what }) => {
                saved += 1;
                if !o.quiet {
                    println!("{name} -> {} ({width}x{height}; {what})", path.display());
                }
            }
            Ok(Outcome::Exists(path)) => {
                existed += 1;
                if !o.quiet {
                    println!("{name}: {} is already there (-o to overwrite)", path.display());
                }
            }
            Ok(Outcome::NotAMission) => not_missions += 1,
            Ok(Outcome::NotASave) => not_saves += 1,
            Err(e) => {
                eprintln!("{name}: {e:#}");
                failed.push(name);
            }
        }
    }
    if !o.quiet && jobs.len() > 1 {
        let mut line = format!("{saved} saved");
        if existed > 0 {
            line += &format!(", {existed} already there");
        }
        if not_missions > 0 {
            line += &format!(", {not_missions} DLLs that aren't missions skipped");
        }
        if not_saves > 0 {
            line += &format!(", {not_saves} .op2 files that aren't saved games skipped");
        }
        line += &format!(", {} failed", failed.len());
        if !failed.is_empty() {
            line += &format!(": {}", failed.join(", "));
        }
        println!("{line}");
    }
    Ok(if failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}
