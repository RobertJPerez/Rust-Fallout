//! Publication retains immutable schemas, never a borrowed world or a cloned
//! content catalogue. Missing context is an explicit refusal before rotation.
use crate::{Error, Limits, Result, World, snapshot::Snapshot, state::DefinitionSchema};
use fallout_data::loaded_scripts::ScriptKey;
use std::{collections::BTreeMap, sync::Arc};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Default)]
pub(super) struct SourceValidation {
    definitions: BTreeMap<ScriptKey, (String, Arc<DefinitionSchema>)>,
    exceeded: Option<&'static str>,
}

impl SourceValidation {
    pub(super) fn capture(world: &World<'_>) -> Self {
        let failure = |reason| Self {
            definitions: BTreeMap::new(),
            exceeded: Some(reason),
        };
        if world.definitions.len() > world.limits.max_instances {
            return failure("publication source definitions");
        }
        let mut locals = 0;
        let mut blocks = 0;
        let mut identity_bytes = 0;
        // Admit the complete cache before allocating its owned index. Schemas
        // from removed instances remain available to validate an older current.
        for (key, schema) in &world.definitions {
            if schema.locals.len() > world.limits.max_locals.saturating_sub(locals) {
                return failure("publication source locals");
            }
            locals += schema.locals.len();
            if schema.blocks.len() > world.limits.max_event_blocks.saturating_sub(blocks) {
                return failure("publication source event blocks");
            }
            blocks += schema.blocks.len();
            let Some(script) = world.catalogue.get(key) else {
                return failure("publication source definition missing");
            };
            for count in [
                key.record.origin_plugin.len(),
                script.handle().version_sha256.len(),
            ] {
                if count
                    > world
                        .limits
                        .max_snapshot_bytes
                        .saturating_sub(identity_bytes)
                {
                    return failure("publication source identity bytes");
                }
                identity_bytes += count;
            }
        }
        Self {
            definitions: world
                .definitions
                .iter()
                .map(|(key, schema)| {
                    let version = world
                        .catalogue
                        .get(key)
                        .expect("admitted immutable definition")
                        .handle()
                        .version_sha256
                        .clone();
                    (key.clone(), (version, Arc::clone(schema)))
                })
                .collect(),
            exceeded: None,
        }
    }

    pub(super) fn check(&self, snapshot: &Snapshot, limits: Limits) -> Result<()> {
        snapshot.validate_intrinsic(limits)?;
        if let Some(reason) = self.exceeded {
            return Err(Error::Capacity(reason));
        }
        snapshot.validate_source_schemas(|handle| {
            let (version, schema) = self
                .definitions
                .get(&handle.key)
                .ok_or(Error::DefinitionChanged)?;
            if version != &handle.version_sha256 {
                return Err(Error::DefinitionChanged);
            }
            Ok(Arc::clone(schema))
        })
    }
}
