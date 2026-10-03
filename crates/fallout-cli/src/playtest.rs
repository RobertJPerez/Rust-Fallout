//! Start the inspection build from an ignored, machine-local configuration.
//! --check validates the handoff without opening a window or changing game files.
use clap::Parser;
use fallout_data::baseline;
use serde::Deserialize;
use serde_json::json;
use std::{
    error::Error,
    io::Read,
    path::PathBuf,
    process::{Command, ExitCode},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Parser)]
#[command(about = "Launch the configured terrain inspection test; gameplay is not implemented")]
struct Options {
    #[arg(long)]
    check: bool,
    #[arg(long)]
    terrain: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema_version: u32,
    install: PathBuf,
    load_order: PathBuf,
    terrain: String,
    texture_repeats_per_quadrant: f32,
}

fn run(options: Options) -> Result<()> {
    let executable = std::env::current_exe()?;
    let root = executable
        .ancestors()
        .find(|p| {
            p.join("Cargo.toml").is_file() && p.join("profiles/nv-inspection-order.json").is_file()
        })
        .ok_or("Place this executable under the Rust-Fallout checkout")?;
    let config_path = root.join("local/playtest.json");
    let mut bytes = Vec::new();
    baseline::open_source(&config_path)
        .map_err(|e| {
            format!("Cannot read local/playtest.json: {e}; see docs/terrain-textured-preview.md")
        })?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
        return Err("Playtest configuration exceeds 64 KiB".into());
    }
    let config: Config = serde_json::from_slice(&bytes)?;
    let terrain = options.terrain.unwrap_or(config.terrain);
    let repeats = config.texture_repeats_per_quadrant;
    if config.schema_version != 1
        || terrain.is_empty()
        || terrain.len() > 256
        || terrain.chars().any(char::is_control)
        || !repeats.is_finite()
        || repeats <= 0.
        || repeats > 64.
    {
        return Err("Invalid playtest schema, cell selector or texture repetition".into());
    }
    let install = config.install.canonicalize()?;
    let order = root.join(config.load_order).canonicalize()?;
    let preview = executable.with_file_name("fallout-preview.exe");
    if !install.join("Data/FalloutNV.esm").is_file() || !order.is_file() || !preview.is_file() {
        return Err("Playtest needs the original NV data, a load-order file and fallout-preview.exe beside this launcher".into());
    }
    let mut command = Command::new(&preview);
    command
        .current_dir(root)
        .arg("--install")
        .arg(&install)
        .arg("--load-order")
        .arg(&order)
        .arg("--terrain")
        .arg(&terrain)
        .arg("--terrain-textures")
        .arg("--terrain-texture-repeat")
        .arg(repeats.to_string());
    if options.check {
        println!(
            "{}",
            json!({"ready":true,"mode":"terrain inspection; no gameplay","preview":preview,"install":install,
            "load_order":order,"terrain":terrain,"texture_repeats_per_quadrant":repeats,"window_launched":false})
        );
        return Ok(());
    }
    println!(
        "Terrain inspection: Tab toggles orbit/fly; WASD moves; Q/E changes height; arrows turn; R resets; Esc exits."
    );
    if !command.status()?.success() {
        return Err("Terrain inspection exited unsuccessfully".into());
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Options::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
