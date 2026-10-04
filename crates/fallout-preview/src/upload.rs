//! Bounded admission to Bevy assets/entities. Decoders have already completed;
//! these budgets count submitted draw bytes, not exact driver VRAM usage.
use crate::{material, model, scene};
use bevy::{ecs::system::SystemParam, prelude::*};
use std::vec::IntoIter;

const FRAME_BYTES: usize = 16 * 1024 * 1024;
const FRAME_ASSETS: usize = 8;
const FRAME_ENTITIES: usize = 128;
const MAX_BYTES: usize = 512 * 1024 * 1024;
const MAX_PARTS: usize = 16_384;
const MAX_ENTITIES: usize = 65_536;

#[derive(SystemParam)]
pub struct Resources<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<material::InspectionMaterial>>,
    images: ResMut<'w, Assets<Image>>,
}

type Template = (Handle<Mesh>, Handle<material::InspectionMaterial>);

pub struct Queue {
    epoch: u64,
    images: IntoIter<Image>,
    models: Vec<IntoIter<model::Part>>,
    instances: IntoIter<scene::Instance>,
    textures: Vec<Handle<Image>>,
    templates: Vec<Vec<Template>>,
    model: usize,
    current: Option<(Entity, usize, usize)>,
    root: Option<Entity>,
    admitted: usize,
    total: usize,
    complete: bool,
    published: bool,
}

fn mesh_bytes(mesh: &Mesh) -> Result<usize, String> {
    mesh.get_vertex_buffer_size()
        .checked_add(mesh.get_index_buffer_bytes().map_or(0, <[u8]>::len))
        .ok_or_else(|| "Draw mesh byte count overflow".into())
}

impl Queue {
    /// Preflight runs on the preparation worker. Oversized work is refused,
    /// rather than starving the queue or submitting an unbounded frame.
    pub fn new(epoch: u64, prepared: scene::Prepared) -> Result<Self, String> {
        let mut bytes = 0usize;
        let mut charge = |amount: usize| -> Result<(), String> {
            if amount > FRAME_BYTES {
                return Err("One draw resource exceeds the 16 MiB upload limit".into());
            }
            bytes = bytes.checked_add(amount).ok_or("Draw bytes overflow")?;
            if bytes > MAX_BYTES {
                return Err("Scene exceeds the 512 MiB draw submission limit".into());
            }
            Ok(())
        };
        if prepared.images.len() > 4096 || prepared.models.len() > 2048 {
            return Err("Scene exceeds texture/model draw limits".into());
        }
        for image in &prepared.images {
            charge(image.data.as_ref().map_or(0, Vec::len))?;
        }
        let mut parts = 0usize;
        for model in &prepared.models {
            parts = parts
                .checked_add(model.parts.len())
                .ok_or("Parts overflow")?;
            if parts > MAX_PARTS {
                return Err("Scene exceeds draw part limit".into());
            }
            for part in &model.parts {
                if part
                    .texture
                    .is_some_and(|index| index >= prepared.images.len())
                {
                    return Err("Draw part references an unavailable source texture".into());
                }
                part.raster.validate().map_err(|error| error.to_string())?;
                charge(mesh_bytes(&part.mesh)?)?;
            }
        }
        let mut entities = 1usize;
        for instance in &prepared.instances {
            let model = prepared
                .models
                .get(instance.model)
                .ok_or("Unknown draw model")?;
            if !instance.transform.is_finite() {
                return Err("Draw instance transform must be finite".into());
            }
            entities = entities
                .checked_add(1 + model.parts.len())
                .ok_or("Draw entities overflow")?;
            if entities > MAX_ENTITIES {
                return Err("Scene exceeds draw entity limit".into());
            }
        }
        let total = parts * 2 + entities + prepared.images.len();
        Ok(Self {
            epoch,
            images: prepared.images.into_iter(),
            templates: (0..prepared.models.len()).map(|_| Vec::new()).collect(),
            models: prepared
                .models
                .into_iter()
                .map(|model| model.parts.into_iter())
                .collect(),
            instances: prepared.instances.into_iter(),
            textures: Vec::new(),
            model: 0,
            current: None,
            root: None,
            admitted: 0,
            total,
            complete: false,
            published: false,
        })
    }

    pub fn status(&self) -> String {
        format!(
            "Uploading draw resources: {}/{} operations",
            self.admitted, self.total
        )
    }

    pub fn advance(
        &mut self,
        commands: &mut Commands,
        resources: &mut Resources,
        expected_epoch: u64,
    ) -> Result<bool, String> {
        if self.epoch != expected_epoch {
            return Err("Discarded stale draw upload".into());
        }
        if self.complete {
            return Ok(true);
        }
        let mut bytes = 0;
        let mut assets = 0;
        let mut entities = 0;
        if self.root.is_none() {
            self.root = Some(
                commands
                    .spawn((Transform::IDENTITY, Visibility::Hidden))
                    .id(),
            );
            entities += 1;
            self.admitted += 1;
        }
        while let Some(image) = self.images.as_slice().first() {
            let amount = image.data.as_ref().map_or(0, Vec::len);
            if assets == FRAME_ASSETS || bytes + amount > FRAME_BYTES {
                return Ok(false);
            }
            bytes += amount;
            assets += 1;
            self.textures.push(
                resources
                    .images
                    .add(self.images.next().expect("peeked image")),
            );
            self.admitted += 1;
        }
        while self.model < self.models.len() {
            while let Some(part) = self.models[self.model].as_slice().first() {
                let amount = mesh_bytes(&part.mesh)?;
                if assets + 2 > FRAME_ASSETS || bytes + amount > FRAME_BYTES {
                    return Ok(false);
                }
                bytes += amount;
                assets += 2;
                let part = self.models[self.model].next().expect("peeked draw part");
                let template = (
                    resources.meshes.add(part.mesh),
                    resources.materials.add(material::adapt(
                        StandardMaterial {
                            base_color: part.color,
                            base_color_texture: part
                                .texture
                                .map(|index| self.textures[index].clone()),
                            ..default()
                        },
                        part.raster,
                    )),
                );
                self.templates[self.model].push(template);
                self.admitted += 2;
            }
            self.model += 1;
        }
        loop {
            if entities == FRAME_ENTITIES {
                return Ok(false);
            }
            if self.current.is_none() {
                let Some(instance) = self.instances.next() else {
                    self.complete = true;
                    return Ok(true);
                };
                let mut parent = commands.spawn((
                    instance.transform,
                    Visibility::Inherited,
                    ChildOf(self.root.expect("draw root")),
                ));
                if let Some(key) = instance.key {
                    let reference = scene::ReferenceView { key };
                    parent.insert((Name::new(reference.label()), reference));
                }
                self.current = Some((parent.id(), instance.model, 0));
                entities += 1;
                self.admitted += 1;
            }
            let (parent, model, index) = self.current.expect("draw instance");
            if let Some((mesh, material)) = self.templates[model].get(index) {
                if entities == FRAME_ENTITIES {
                    return Ok(false);
                }
                commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    ChildOf(parent),
                ));
                self.current = Some((parent, model, index + 1));
                entities += 1;
                self.admitted += 1;
            } else {
                self.current = None;
            }
        }
    }

    /// Final visibility is a separate, once-only admission. CELL callers execute
    /// this short command enqueue inside CellResidency::publish_render.
    pub fn publish(&mut self, commands: &mut Commands, expected_epoch: u64) -> Result<(), String> {
        if !self.complete || self.published || self.epoch != expected_epoch {
            return Err("Draw publication needs a current, complete, unpublished scene".into());
        }
        commands
            .entity(self.root.ok_or("Draw root unavailable")?)
            .insert(Visibility::Inherited);
        self.published = true;
        Ok(())
    }

    /// Host cancellation removes only this queue's entities and asset handles.
    /// Each scene is preflight bounded; incremental disposal follows in VIEW08.
    pub fn dispose(&mut self, commands: &mut Commands, resources: &mut Resources) {
        if let Some(root) = self.root.take() {
            commands.entity(root).despawn();
        }
        for template in self.templates.iter_mut().flat_map(|parts| parts.drain(..)) {
            resources.meshes.remove(template.0.id());
            resources.materials.remove(template.1.id());
        }
        for image in self.textures.drain(..) {
            resources.images.remove(image.id());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource)]
    struct TestQueue(Queue);
    #[derive(Resource)]
    struct Epoch(u64);

    fn advance(
        mut commands: Commands,
        mut queue: ResMut<TestQueue>,
        mut assets: Resources,
        epoch: Res<Epoch>,
    ) {
        match queue.0.advance(&mut commands, &mut assets, epoch.0) {
            Ok(true) if !queue.0.published => {
                queue.0.publish(&mut commands, epoch.0).unwrap();
            }
            Err(_) => queue.0.dispose(&mut commands, &mut assets),
            _ => {}
        }
    }

    fn app(prepared: scene::Prepared) -> App {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<material::InspectionMaterial>>()
            .init_resource::<Assets<Image>>()
            .insert_resource(TestQueue(Queue::new(7, prepared).unwrap()))
            .insert_resource(Epoch(7))
            .add_systems(Update, advance);
        app
    }

    #[test]
    fn actual_assets_are_batched_hidden_until_complete_and_disposed_on_stale_epoch() {
        let (mut prepared, _) = crate::fixture::prepare().unwrap();
        let source_key = fallout_data::identity::resolve_form(
            fallout_data::identity::ProfileId::NvOriginal,
            "Authored.esm",
            &[],
            0x1234,
        )
        .unwrap()
        .unwrap();
        prepared.instances[0].key = Some(source_key.clone());
        let parts = prepared
            .models
            .iter()
            .map(|model| model.parts.len())
            .sum::<usize>();
        let mut app = app(prepared);
        app.update();
        assert!(app.world().resource::<Assets<Mesh>>().len() <= 4);
        let root = app.world().resource::<TestQueue>().0.root.unwrap();
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Hidden
        );
        for _ in 0..100 {
            if app.world().resource::<TestQueue>().0.complete {
                break;
            }
            let before = app.world().resource::<Assets<Mesh>>().len();
            // Bevy 0.19 len() counts allocated indices, including unspawned
            // reservations. The frame budget concerns actual live entities.
            let entities = app.world().entities().count_spawned();
            app.update();
            assert!(app.world().resource::<Assets<Mesh>>().len() - before <= 4);
            assert!(app.world().entities().count_spawned() - entities <= FRAME_ENTITIES as u32);
        }
        assert!(app.world().resource::<TestQueue>().0.complete);
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), parts);
        let mut references = app.world_mut().query::<(&scene::ReferenceView, &Name)>();
        let (reference, name) = references.single(app.world()).unwrap();
        assert_eq!(reference.key, source_key);
        assert_eq!(name.as_str(), "authored.esm:001234");
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Inherited
        );
        app.world_mut().resource_mut::<Epoch>().0 = 8;
        app.update();
        assert!(app.world().get_entity(root).is_err());
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
        assert!(
            app.world()
                .resource::<Assets<material::InspectionMaterial>>()
                .is_empty()
        );
        assert!(app.world().resource::<Assets<Image>>().is_empty());
    }

    #[test]
    fn malformed_and_oversized_draw_work_is_refused_before_admission() {
        let (mut prepared, _) = crate::fixture::prepare().unwrap();
        prepared.models[0].parts[0].texture = Some(prepared.images.len());
        assert!(Queue::new(7, prepared).is_err());
        let (mut prepared, _) = crate::fixture::prepare().unwrap();
        prepared.images[0].data = Some(vec![0; FRAME_BYTES + 1]);
        assert!(Queue::new(7, prepared).is_err());
        let (prepared, _) = crate::fixture::prepare().unwrap();
        let mut app = app(prepared);
        app.world_mut().resource_mut::<Epoch>().0 = 8;
        app.update();
        assert!(app.world().resource::<TestQueue>().0.root.is_none());
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    }
}
