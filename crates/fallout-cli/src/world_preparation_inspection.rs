//! Read-only requests over protected winning world/content sources.
use super::{Result, inspection_input::Order};
use fallout_data::{
    condition_operands::Signatures,
    identity::FormKey,
    loaded_scripts::{Catalogue, Limits as ScriptLimits},
    world::conversation::{DialogueSources, Limits},
};
use serde_json::{Value, json};
use std::path::Path;

pub(super) struct Input {
    pub topic: FormKey,
    pub info: FormKey,
    pub speaker: Option<FormKey>,
    pub bind_result_fragments: bool,
}
pub(super) fn conversation(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    input: Input,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let sources = DialogueSources::build(&mut store, Limits::default())?;
    let request = sources.request(input.topic, input.info, input.speaker)?;
    // This is an exact source inspection, not a native function metadata capture.
    // Unknown function signatures remain explicit in each existing CTDA binding.
    let prepared = sources.prepare(&mut store, &request, &Signatures::new(), Limits::default())?;
    let fragments = if input.bind_result_fragments {
        let catalogue = Catalogue::load(&mut store, ScriptLimits::default(), |_, _| Ok(()))?;
        Some(prepared.metadata().fragments.iter().map(|fragment| {
            let loaded = fragment.resolve(&catalogue)?;
            Ok(json!({"handle":loaded.handle(),"version":loaded.version(),"owner":loaded.owner(),
                "issues":loaded.issues(),"execution_admitted":false}))
        }).collect::<fallout_data::Result<Vec<_>>>()?)
    } else {
        None
    };
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":order.names,
        "load_order_sha256":order.sha256,"plugins":store.source_receipts()?,
        "membership_metadata_bytes":sources.retained_bytes(),"retained_conversation_bytes":prepared.retained_bytes(),
        "conversation":prepared.metadata(),"loaded_fragments":fragments,
        "condition_signatures_supplied":false,"runtime_ready":false,"retail_parity_accepted":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // Private ignored fixtures, kept for review rather than adding a dependency
    // or deleting an unrelated temporary tree during a shared team run.
    fn directory() -> std::path::PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/world-conversation-cli-fixtures");
        fs::create_dir_all(&root).unwrap();
        let mut ordinal = 0;
        loop {
            let path = root.join(format!("{}-{ordinal}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => ordinal += 1,
                Err(error) => panic!("private conversation fixture: {error}"),
            }
        }
    }

    fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
    }
    fn record(kind: &[u8; 4], form: u32, body: &[u8]) -> Vec<u8> {
        [
            kind.as_slice(),
            &(body.len() as u32).to_le_bytes(),
            &[0; 4],
            &form.to_le_bytes(),
            &[0; 8],
            body,
        ]
        .concat()
    }
    fn fixture(root: &Path) {
        fs::create_dir(root.join("Data")).unwrap();
        let mut schr = [0; 20];
        schr[8..12].copy_from_slice(&4_u32.to_le_bytes());
        let info = record(
            b"INFO",
            0x300,
            &[
                field(b"TRDT", &[0; 24]),
                field(b"NAM1", b"authored_fixture_line\0"),
                field(b"SCHR", &schr),
                field(b"SCDA", &[0x1d, 0, 0, 0]),
            ]
            .concat(),
        );
        let group = [
            b"GRUP".as_slice(),
            &(info.len() as u32 + 24).to_le_bytes(),
            &0x100_u32.to_le_bytes(),
            &7_i32.to_le_bytes(),
            &[0; 8],
            &info,
        ]
        .concat();
        fs::write(
            root.join("Data/Base.esm"),
            [
                record(
                    b"TES4",
                    0,
                    &field(
                        b"HEDR",
                        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                    ),
                ),
                record(b"DIAL", 0x100, &field(b"FULL", b"authored_fixture_topic\0")),
                group,
            ]
            .concat(),
        )
        .unwrap();
        fs::write(root.join("order.json"), b"[\"Base.esm\"]").unwrap();
    }
    fn input(info: &str) -> Input {
        Input {
            topic: crate::parse_cell_key("Base.esm:100").unwrap(),
            info: crate::parse_cell_key(info).unwrap(),
            speaker: None,
            bind_result_fragments: true,
        }
    }
    #[test]
    fn cli_consumer_prepares_exact_sources_and_binds_existing_loaded_unit() {
        let directory = directory();
        fixture(&directory);
        let report = conversation(
            &directory,
            &directory.join("order.json"),
            None,
            input("Base.esm:300"),
        )
        .unwrap();
        assert_eq!(report["conversation"]["info"]["key"]["local_id"], 0x300);
        assert_eq!(
            report["conversation"]["responses"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            report["loaded_fragments"][0]["handle"]["key"]["record"]["local_id"],
            0x300
        );
        assert_eq!(report["loaded_fragments"][0]["execution_admitted"], false);
        assert_eq!(report["runtime_ready"], false);
        assert_eq!(report["condition_signatures_supplied"], false);
        assert_eq!(report["retail_parity_accepted"], false);
        assert!(!report.to_string().contains("authored_fixture"));
    }
    #[test]
    fn cli_consumer_refuses_info_outside_requested_winning_topic() {
        let directory = directory();
        fixture(&directory);
        assert!(
            conversation(
                &directory,
                &directory.join("order.json"),
                None,
                input("Base.esm:100")
            )
            .is_err()
        );
    }
}
