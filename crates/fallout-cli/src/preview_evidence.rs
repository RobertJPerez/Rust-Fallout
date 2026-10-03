//! GPU smoke checks retain captures locally. Pixel occupancy is a smoke check,
//! not a comparison with the retail renderer or proof of camera correctness.
use super::{Result, digest, json_file, run_logged};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

pub fn run(
    root: &Path,
    directory: &Path,
    binary: &Path,
    install: &Path,
    sha: &str,
) -> Result<Value> {
    let folder = directory.join("gpu");
    fs::create_dir(&folder)?;
    let mut captures = Vec::new();
    for cell in [
        "Goodsprings",
        "TownCenter",
        "NVDLC02PineCreek",
        "NVDLC03SLVillage",
        "NVDLC04DivideEast",
        "GSDocMitchellHouse",
    ] {
        let terrain = cell != "GSDocMitchellHouse";
        let png = folder.join(format!("{cell}.png"));
        let report = folder.join(format!("{cell}.json"));
        let mut command = Command::new(binary);
        command
            .current_dir(root)
            .arg("--install")
            .arg(install)
            .arg(if terrain { "--terrain" } else { "--cell" })
            .arg(cell)
            .arg("--load-order")
            .arg(root.join("profiles/nv-inspection-order.json"))
            .arg("--headless")
            .arg("--capture")
            .arg(&png)
            .arg("--report")
            .arg(&report);
        if !terrain {
            command.args([
                "--camera-position",
                "2130",
                "2130",
                "7440",
                "--camera-look-at",
                "1883",
                "1763",
                "7420",
            ]);
        }
        run_logged(command, &folder.join(format!("{cell}.log")))?;
        let report_value = json_file(&report)?;
        if terrain
            && (report_value["vertices"] != 1089
                || report_value["triangles"] != 2048
                || report_value["retail_parity_accepted"] != false)
        {
            return Err("Terrain GPU fixture geometry or acceptance differs".into());
        }
        if !terrain && report_value["rendered_references"] != 400 {
            return Err("Interior preview regression count differs".into());
        }
        let pixels = image::ImageReader::open(&png)?
            .with_guessed_format()?
            .decode()?
            .to_rgba8();
        let (width, height) = pixels.dimensions();
        if (width, height) != (1280, 900) {
            return Err("Unexpected GPU capture size".into());
        }
        let background = pixels.get_pixel(0, 0);
        let foreground = pixels.pixels().filter(|p| p != &background).count();
        if foreground < 1000 {
            return Err("GPU capture is empty or nearly empty".into());
        }
        captures.push(json!({"cell_editor_id":cell, "mode":if terrain {"terrain"} else {"interior regression"},
            "width":width,"height":height,"pixels_differing_from_top_left":foreground,
            "capture_sha256":digest(&png)?,"report_sha256":digest(&report)?,"log_sha256":digest(&folder.join(format!("{cell}.log")))?,
            "vertices":report_value["vertices"],"triangles":report_value["triangles"],"color_mode":report_value["color_mode"],
            "source_origin":report_value["source_origin"],"relative_view_bounds":report_value["relative_view_bounds"]}));
    }
    Ok(
        json!({"schema_version":1,"checkpoint":11,"preview_binary_sha256":sha,"build_profile":"debug",
        "captures":captures,"captures_kept_local":true,"retail_parity_accepted":false,
        "checks":"Successful GPU capture, expected source geometry counts, 1280x900 PNG and nonempty pixel occupancy; interior assembly count regression",
        "scope":"Five source terrain cells with authored colors or explicitly absent VCLR plus one interior regression; unlit inspection; no retail reference images or gameplay acceptance"}),
    )
}
