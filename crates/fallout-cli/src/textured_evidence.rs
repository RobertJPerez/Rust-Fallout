//! GPU evidence for authored diffuse inspection. Captures remain local; only
//! provenance, measurements and original synthetic expectations are published.
use super::{Result, digest, json_file, run_logged, run_logged_status};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn command(
    root: &Path,
    binary: &Path,
    install: &Path,
    folder: &Path,
    name: &str,
    terrain: bool,
    textured: bool,
) -> Command {
    let mut command = Command::new(binary);
    command
        .current_dir(root)
        .arg("--install")
        .arg(install)
        .arg(if terrain { "--terrain" } else { "--cell" })
        .arg(name)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--headless")
        .arg("--capture")
        .arg(folder.join(format!("{name}.png")))
        .arg("--report")
        .arg(folder.join(format!("{name}.json")));
    if textured {
        command.args(["--terrain-textures", "--terrain-texture-repeat", "4"]);
    }
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
    command
}

fn capture(folder: &Path, name: &str) -> Result<Value> {
    let png = folder.join(format!("{name}.png"));
    let pixels = image::ImageReader::open(&png)?.decode()?.to_rgba8();
    if pixels.dimensions() != (1280, 900) {
        return Err("Unexpected textured capture dimensions".into());
    }
    let background = pixels.get_pixel(0, 0);
    let foreground = pixels.pixels().filter(|p| p != &background).count();
    if foreground < 1000 {
        return Err("Empty textured GPU capture".into());
    }
    Ok(
        json!({"cell_editor_id":name,"width":1280,"height":900,"pixels_differing_from_top_left":foreground,
        "capture_sha256":digest(&png)?,"report_sha256":digest(&folder.join(format!("{name}.json")))?,
        "log_sha256":digest(&folder.join(format!("{name}.log")))?}),
    )
}

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
    for name in [
        "Goodsprings",
        "GoodspringsSource",
        "NVDLC02PineCreek",
        "TownCenter",
        "GSDocMitchellHouse",
    ] {
        let terrain = name != "GSDocMitchellHouse";
        let textured = terrain && name != "TownCenter";
        run_logged(
            command(root, binary, install, &folder, name, terrain, textured),
            &folder.join(format!("{name}.log")),
        )?;
        let report = json_file(&folder.join(format!("{name}.json")))?;
        if report["retail_parity_accepted"] != false {
            return Err("Preview report must explicitly mark retail parity as unaccepted".into());
        }
        if terrain && (report["vertices"] != 1089 || report["triangles"] != 2048) {
            return Err("Terrain source geometry regression".into());
        }
        if !terrain && report["rendered_references"] != 400 {
            return Err("Interior assembly regression".into());
        }
        let mut receipt = capture(&folder, name)?;
        receipt["mode"] = if textured {
            "authored diffuse inspection"
        } else if terrain {
            "vertex-color regression"
        } else {
            "interior regression"
        }
        .into();
        if textured {
            let cli = json_file(&directory.join(name).join("compared.json"))?;
            if report["terrain"]["landscapes"][0]["decoded_sha256"]
                != cli["landscapes"][0]["decoded_sha256"]
                || report["textured"]["blend_maps"] != cli["blend_maps"][0]["blends"]
                || report["textured"]["texture_repeats_per_quadrant"] != 4.
            {
                return Err("GPU terrain inputs differ from independently compared inputs".into());
            }
            let textures = report["textured"]["diffuse_textures"]
                .as_array()
                .ok_or("Missing GPU diffuse textures")?;
            if textures.is_empty() {
                return Err("Textured fixture uploaded no source textures".into());
            }
            for texture in textures {
                let source = cli["texture_dependencies"]["assets"]
                    .as_array()
                    .ok_or("Missing source assets")?
                    .iter()
                    .find(|a| a["path"] == texture["path"])
                    .ok_or("GPU texture is not in the selected dependency chain")?;
                if source["sha256"] != texture["sha256"] {
                    return Err("GPU diffuse bytes differ from archive oracle inputs".into());
                }
            }
            for key in [
                "layer_draws",
                "drawn_vertices",
                "drawn_triangles",
                "texture_repeats_per_quadrant",
            ] {
                receipt[key] = report["textured"][key].clone();
            }
            receipt["diffuse_textures"] = textures.len().into();
            receipt["overfull_vertices"] =
                report["textured"]["blend_maps"]["overfull_vertices"].clone();
            receipt["source_origin"] = report["source_origin"].clone();
        }
        captures.push(receipt);
    }
    let mut rejected = Vec::new();
    let failures = folder.join("unsupported");
    fs::create_dir(&failures)?;
    for (name, expected) in [
        ("TownCenter", "missing base layers"),
        ("NVDLC03SLVillage", "unapplied NULL default layers"),
        ("NVDLC04DivideEast", "unapplied NULL default layers"),
    ] {
        let log = failures.join(format!("{name}.log"));
        let output = run_logged_status(
            command(root, binary, install, &failures, name, true, true),
            &log,
            1,
        )?;
        if !String::from_utf8(output.stderr)?.contains(expected)
            || failures.join(format!("{name}.png")).exists()
            || failures.join(format!("{name}.json")).exists()
        {
            return Err("Incomplete material fixture did not fail before rendering".into());
        }
        rejected.push(json!({"cell_editor_id":name,"reason":expected,"exit_code":1,"capture_created":false,"log_sha256":digest(&log)?}));
    }
    let name = "material-fixture";
    let mut command = Command::new(binary);
    command
        .current_dir(root)
        .args(["--material-fixture", "--headless", "--capture"])
        .arg(folder.join(format!("{name}.png")))
        .arg("--report")
        .arg(folder.join(format!("{name}.json")));
    run_logged(command, &folder.join(format!("{name}.log")))?;
    let material = json_file(&folder.join(format!("{name}.json")))?;
    let cases = material["cases"]
        .as_array()
        .ok_or("Missing GPU expectations")?;
    if cases.len() != 64
        || material["all_passed"] != true
        || material["retail_parity_accepted"] != false
        || cases.iter().any(|c| c["passed"] != true)
    {
        return Err("Material/terrain GPU numeric comparison failed".into());
    }
    let mut receipt = capture(&folder, name)?;
    receipt["mode"] = "original synthetic material and terrain weight expectations".into();
    captures.push(receipt);
    Ok(
        json!({"schema_version":1,"checkpoint":14,"preview_binary_sha256":sha,"build_profile":"debug","captures":captures,
        "unsupported_material_fixtures":rejected,"material_gpu_expectations":material,"gpu_expectations_passed":64,
        "captures_kept_local":true,"retail_parity_accepted":false,
        "scope":"Three authored diffuse terrain captures bound to independently compared body/weight/texture bytes, interior and vertex-color regressions, 64 original numeric GPU expectations; no retail reference images, defaults, normal/specular shader or gameplay acceptance"}),
    )
}
