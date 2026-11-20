//! OP2Timelapse: an animated GIF of a game's whole-map images (OP2TestHarness's `map_<tick>.png`, written every N
//! ticks with `--map-every N`), or of any PNGs of one size, each labelled with its tick.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

mod gif;

const USAGE: &str = "\
OP2Timelapse: an animated GIF of a game's whole-map images

usage: OP2Timelapse [options] OUT.gif FRAME.png|FOLDER ...

  FRAME    an image (PNG, or anything else OP2Imager reads); all must be one size
  FOLDER   its map_<tick>.png files (OP2TestHarness's whole-map images, --map-every), in tick order; a folder without
           them gives its .png files by name

  --delay CS     each frame's time on screen, in hundredths of a second; default 40
  --last CS      the last frame's, before it loops; default four times --delay
  --title TEXT   put before each frame's label (e.g. the mission's name)
  --no-label     no label bar: the frames as they are
  -q, --quiet    print only errors
  -h, --help     this text
  -V, --version  the version

Each frame is labelled `TITLE  TICK N`, its tick from its name (map_0012000.png is tick 12000), or with the file's
name. The GIF loops and shares one palette of 253 colours across its frames (median cut, so team colours and lava
keep their colours); each frame after the first stores only the pixels that changed.
Example:
  OP2Timelapse --title E05 e05.gif runs\\e05";

struct Options {
    out: PathBuf,
    inputs: Vec<PathBuf>,
    delay: u16,
    last: Option<u16>,
    title: String,
    label: bool,
    quiet: bool,
}

fn parse_args() -> Result<Option<Options>> {
    let mut o = Options { out: PathBuf::new(), inputs: Vec::new(), delay: 40, last: None, title: String::new(), label: true, quiet: false };
    let mut paths = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        if !text.starts_with('-') || text == "-" {
            paths.push(PathBuf::from(arg));
            continue;
        }
        let flag = text.to_ascii_lowercase();
        let mut value = |what: &str| args.next().map(|v| v.to_string_lossy().into_owned()).with_context(|| format!("{flag} needs {what}"));
        let centis = |v: String, flag: &str| v.parse::<u16>().ok().filter(|n| (1..=6000).contains(n)).with_context(|| format!("{flag}: 1 to 6000 hundredths of a second"));
        match flag.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "-v" | "--version" => {
                println!("OP2Timelapse {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--delay" => o.delay = centis(value("a number")?, "--delay")?,
            "--last" => o.last = Some(centis(value("a number")?, "--last")?),
            "--title" => o.title = value("a text")?,
            "--no-label" => o.label = false,
            "-q" | "--quiet" => o.quiet = true,
            _ => bail!("unknown option {text}\n\n{USAGE}"),
        }
    }
    if paths.len() < 2 {
        bail!("give the GIF to write and its frames\n\n{USAGE}");
    }
    o.out = paths.remove(0);
    o.inputs = paths;
    Ok(Some(o))
}

/// The tick in a whole-map image's name (`map_0012000.png` -> 12000).
fn tick_of(path: &Path) -> Option<u64> {
    path.file_stem()?.to_str()?.strip_prefix("map_")?.parse().ok()
}

/// A folder's frames: its `map_<tick>.png` in tick order, else its `.png` files by name.
fn folder_frames(dir: &Path) -> Result<Vec<PathBuf>> {
    let pngs: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
        .collect();
    let mut maps: Vec<PathBuf> = pngs.iter().filter(|p| tick_of(p).is_some()).cloned().collect();
    if !maps.is_empty() {
        maps.sort_by_key(|p| tick_of(p));
        return Ok(maps);
    }
    let mut all = pngs;
    all.sort_by_key(|p| p.file_name().map(|f| f.to_ascii_lowercase()));
    Ok(all)
}

fn run() -> Result<ExitCode> {
    let Some(o) = parse_args()? else { return Ok(ExitCode::SUCCESS) };
    let mut files = Vec::new();
    for input in &o.inputs {
        if input.is_dir() {
            let found = folder_frames(input)?;
            if found.is_empty() {
                bail!("{}: no .png files in it", input.display());
            }
            files.extend(found);
        } else {
            files.push(input.clone());
        }
    }
    let mut frames = Vec::with_capacity(files.len());
    for f in &files {
        let img = image::open(f).with_context(|| format!("reading {}", f.display()))?.into_rgb8();
        if let Some(first) = frames.first().map(image::RgbImage::dimensions)
            && img.dimensions() != first
        {
            bail!("{}: {}x{}, not {}x{} as the first frame", f.display(), img.width(), img.height(), first.0, first.1);
        }
        frames.push(img);
    }
    let labels: Vec<String> = if o.label {
        files
            .iter()
            .map(|f| {
                let what = match tick_of(f) {
                    Some(t) => format!("TICK {t}"),
                    None => f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                };
                if o.title.is_empty() { what } else { format!("{}  {what}", o.title) }
            })
            .collect()
    } else {
        Vec::new()
    };
    let opts = gif::Options { delay: o.delay, last_delay: o.last.unwrap_or(o.delay.saturating_mul(4).min(6000)) };
    let e = gif::encode(&frames, &labels, &opts);
    if let Some(dir) = o.out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(&o.out, &e.gif).with_context(|| format!("writing {}", o.out.display()))?;
    if !o.quiet {
        println!(
            "{}: {} frames, {}x{}, {} bytes; palette: pixels {:.1} levels off on average, {} of {} more than 24",
            o.out.display(),
            frames.len(),
            e.width,
            e.height,
            e.gif.len(),
            e.mean_error,
            e.far_off,
            e.pixels
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("OP2Timelapse: {e:#}");
            ExitCode::FAILURE
        }
    }
}
