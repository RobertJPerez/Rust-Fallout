//! Bounded admission to Bevy assets/entities. Decoders have already completed;
//! these budgets count submitted draw bytes, not exact driver VRAM usage.
use crate::{
    material, model, scene,
    ui::{images::ImageView, rectangles::TileView},
};
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
    tile_views: IntoIter<TileView>,
    current_tile: Option<TileView>,
    image_view: Option<ImageView>,
    textures: Vec<Handle<Image>>,
    templates: Vec<Vec<Template>>,
    model: usize,
    current: Option<(Entity, usize, usize)>,
    root: Option<Entity>,
    admitted: usize,
    total: usize,
    complete: bool,
    published: bool,
    retiring: bool,
    owned_entities: Vec<Entity>,
    retiring_model: usize,
    retiring_cpu_model: usize,
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
            tile_views: Vec::new().into_iter(),
            current_tile: None,
            image_view: None,
            textures: Vec::new(),
            model: 0,
            current: None,
            root: None,
            admitted: 0,
            total,
            complete: false,
            published: false,
            retiring: false,
            owned_entities: Vec::new(),
            retiring_model: 0,
            retiring_cpu_model: 0,
        })
    }

    /// Source labels follow the same instance admission and retirement budgets.
    pub fn new_tiles(
        epoch: u64,
        prepared: scene::Prepared,
        views: Vec<TileView>,
    ) -> Result<Self, String> {
        if views.len() != prepared.instances.len() || views.len() > 256 {
            return Err("Tile labels must match every bounded draw instance".into());
        }
        let mut nodes = std::collections::BTreeSet::new();
        for (view, instance) in views.iter().zip(&prepared.instances) {
            if view.epoch != epoch
                || view.span.start >= view.span.end
                || instance.key.is_some()
                || instance.canonical.is_some()
                || !nodes.insert(view.node)
            {
                return Err("Tile label epoch/identity differs or is duplicated".into());
            }
            view.source.validate().map_err(|e| e.to_string())?;
            if views.first().is_some_and(|first| {
                !std::sync::Arc::ptr_eq(&first.source, &view.source)
                    || first.root_node != view.root_node
            }) {
                return Err("Tile labels require one exact source receipt and subtree".into());
            }
        }
        let mut queue = Self::new(epoch, prepared)?;
        queue.tile_views = views.into_iter();
        Ok(queue)
    }

    pub fn status(&self) -> String {
        format!(
            "Uploading draw resources: {}/{} operations",
            self.admitted, self.total
        )
    }

    pub fn new_image(
        epoch: u64,
        prepared: scene::Prepared,
        view: ImageView,
    ) -> Result<Self, String> {
        if prepared.instances.len() != 1
            || prepared.models.len() != 1
            || prepared.images.len() != 1
            || prepared.models[0].parts.len() != 1
            || prepared.models[0].parts[0].texture != Some(0)
        {
            return Err("Image labels require one exact image/model/part/instance".into());
        }
        view.validate().map_err(|e| e.to_string())?;
        let mut queue = Self::new_tiles(epoch, prepared, vec![view.tile.clone()])?;
        queue.image_view = Some(view);
        Ok(queue)
    }

    pub fn advance(
        &mut self,
        commands: &mut Commands,
        resources: &mut Resources,
        expected_epoch: u64,
    ) -> Result<bool, String> {
        if self.retiring || self.epoch != expected_epoch {
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
            self.owned_entities.push(self.root.expect("new draw root"));
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
                    instance.visibility,
                    ChildOf(self.root.expect("draw root")),
                ));
                if let Some(key) = instance.key {
                    let reference = scene::ReferenceView {
                        key,
                        canonical: instance.canonical,
                    };
                    parent.insert((Name::new(reference.label()), reference));
                }
                self.current_tile = self.tile_views.next();
                if let Some(tile) = &self.current_tile {
                    parent.insert(tile.clone());
                }
                if let Some(image) = &self.image_view {
                    parent.insert(image.clone());
                }
                self.current = Some((parent.id(), instance.model, 0));
                self.owned_entities.push(parent.id());
                entities += 1;
                self.admitted += 1;
            }
            let (parent, model, index) = self.current.expect("draw instance");
            if let Some((mesh, material)) = self.templates[model].get(index) {
                if entities == FRAME_ENTITIES {
                    return Ok(false);
                }
                let mut child = commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    ChildOf(parent),
                ));
                if let Some(tile) = &self.current_tile {
                    child.insert(tile.clone());
                }
                if let Some(image) = &self.image_view {
                    child.insert(image.clone());
                }
                let child = child.id();
                self.owned_entities.push(child);
                self.current = Some((parent, model, index + 1));
                entities += 1;
                self.admitted += 1;
            } else {
                self.current = None;
                self.current_tile = None;
            }
        }
    }

    /// Final visibility is a separate, once-only admission. CELL callers execute
    /// this short command enqueue inside CellResidency::publish_render.
    pub fn publish(&mut self, commands: &mut Commands, expected_epoch: u64) -> Result<(), String> {
        if self.retiring || !self.complete || self.published || self.epoch != expected_epoch {
            return Err("Draw publication needs a current, complete, unpublished scene".into());
        }
        commands
            .entity(self.root.ok_or("Draw root unavailable")?)
            .insert(Visibility::Inherited);
        self.published = true;
        Ok(())
    }

    pub fn retiring(&self) -> bool {
        self.retiring
    }

    pub fn disposal_status(&self) -> String {
        format!(
            "Unloading owned draw resources: {} entities, {} templates, {} images",
            self.owned_entities.len(),
            self.templates.iter().map(Vec::len).sum::<usize>(),
            self.textures.len()
        )
    }

    /// Close upload/publication first. Keep all submitted and unsubmitted work
    /// owned until bounded retirement completes. Root visibility closes in this
    /// update; reverse creation order removes children before their parents so
    /// relationship cascade cannot silently bypass the entity budget.
    pub fn dispose(&mut self, commands: &mut Commands, resources: &mut Resources) -> bool {
        if !self.retiring {
            self.retiring = true;
            self.current = None;
            self.current_tile = None;
            self.image_view = None;
            if let Some(root) = self.root {
                commands.entity(root).insert(Visibility::Hidden);
            }
        }
        let mut entities = 0;
        while entities < FRAME_ENTITIES {
            if let Some(entity) = self.owned_entities.pop() {
                commands.entity(entity).despawn();
            } else {
                if self.instances.next().is_none() {
                    break;
                }
                self.tile_views.next();
            }
            entities += 1;
        }
        let mut assets = 0;
        let mut bytes = 0;
        while self.retiring_model < self.templates.len() {
            let templates = &mut self.templates[self.retiring_model];
            while let Some(template) = templates.last() {
                let amount = resources.meshes.get(&template.0).map_or(0, |mesh| {
                    mesh_bytes(mesh).expect("preflight mesh byte count")
                });
                if assets + 2 > FRAME_ASSETS || bytes + amount > FRAME_BYTES {
                    return false;
                }
                let template = templates.pop().expect("peeked owned template");
                resources.meshes.remove(template.0.id());
                resources.materials.remove(template.1.id());
                assets += 2;
                bytes += amount;
            }
            self.retiring_model += 1;
        }
        while let Some(image) = self.textures.last() {
            let amount = resources
                .images
                .get(image)
                .and_then(|image| image.data.as_ref())
                .map_or(0, Vec::len);
            if assets == FRAME_ASSETS || bytes + amount > FRAME_BYTES {
                return false;
            }
            resources
                .images
                .remove(self.textures.pop().expect("peeked owned image").id());
            assets += 1;
            bytes += amount;
        }
        while let Some(image) = self.images.as_slice().first() {
            let amount = image.data.as_ref().map_or(0, Vec::len);
            if assets == FRAME_ASSETS || bytes + amount > FRAME_BYTES {
                return false;
            }
            self.images.next();
            assets += 1;
            bytes += amount;
        }
        while self.retiring_cpu_model < self.models.len() {
            let parts = &mut self.models[self.retiring_cpu_model];
            while let Some(part) = parts.as_slice().first() {
                let amount = mesh_bytes(&part.mesh).expect("preflight mesh byte count");
                if assets == FRAME_ASSETS || bytes + amount > FRAME_BYTES {
                    return false;
                }
                parts.next();
                assets += 1;
                bytes += amount;
            }
            self.retiring_cpu_model += 1;
        }
        let complete = self.owned_entities.is_empty() && self.instances.len() == 0;
        if complete {
            self.root = None;
        }
        complete
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
            Err(_) => {
                queue.0.dispose(&mut commands, &mut assets);
            }
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
        if let Some(visible) = app.world().get::<Visibility>(root) {
            assert_eq!(*visible, Visibility::Hidden);
        }
        assert!(app.world().resource::<TestQueue>().0.retiring());
        while !app.world().resource::<Assets<Mesh>>().is_empty()
            || !app
                .world()
                .resource::<Assets<material::InspectionMaterial>>()
                .is_empty()
            || !app.world().resource::<Assets<Image>>().is_empty()
        {
            let before = app.world().resource::<Assets<Mesh>>().len();
            app.update();
            assert!(before - app.world().resource::<Assets<Mesh>>().len() <= FRAME_ASSETS / 2);
        }
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
    fn retirement_hides_immediately_bounds_each_frame_and_preserves_unrelated_resources() {
        let (mut prepared, _) = crate::fixture::prepare().unwrap();
        prepared.instances = (0..64)
            .map(|_| scene::Instance {
                model: 0,
                transform: Transform::IDENTITY,
                key: None,
                visibility: Visibility::Inherited,
                canonical: None,
            })
            .collect();
        let mut app = app(prepared);
        let unrelated = app
            .world_mut()
            .spawn((Transform::IDENTITY, Visibility::Inherited))
            .id();
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Cuboid::from_size(Vec3::ONE)));
        for _ in 0..200 {
            app.update();
            if app.world().resource::<TestQueue>().0.published {
                break;
            }
        }
        assert!(app.world().resource::<TestQueue>().0.published);
        let root = app.world().resource::<TestQueue>().0.root.unwrap();
        let before = app.world().entities().count_spawned();
        app.world_mut().resource_mut::<Epoch>().0 = 8;
        app.update();
        assert!(before - app.world().entities().count_spawned() <= FRAME_ENTITIES as u32);
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Hidden
        );
        assert!(app.world().resource::<TestQueue>().0.retiring());
        // Returning to the original epoch cannot revive disposal or publish it.
        app.world_mut().resource_mut::<Epoch>().0 = 7;
        for _ in 0..200 {
            let before = app.world().entities().count_spawned();
            let assets = app.world().resource::<Assets<Mesh>>().len();
            app.update();
            assert!(before - app.world().entities().count_spawned() <= FRAME_ENTITIES as u32);
            assert!(assets - app.world().resource::<Assets<Mesh>>().len() <= FRAME_ASSETS / 2);
            assert!(app.world().get_entity(unrelated).is_ok());
            assert!(app.world().resource::<Assets<Mesh>>().get(&mesh).is_some());
            if app.world().resource::<TestQueue>().0.root.is_none() {
                break;
            }
        }
        assert!(app.world().resource::<TestQueue>().0.root.is_none());
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
        assert!(
            app.world()
                .resource::<Assets<material::InspectionMaterial>>()
                .is_empty()
        );
        assert!(app.world().resource::<Assets<Image>>().is_empty());
    }

    #[test]
    fn cancellation_before_upload_retires_valid_prepared_images_at_the_exact_byte_boundary() {
        let (mut prepared, _) = crate::fixture::prepare().unwrap();
        for model in &mut prepared.models {
            for part in &mut model.parts {
                part.texture = None;
            }
        }
        prepared.images = (0..3)
            .map(|_| {
                Image::new_fill(
                    bevy::render::render_resource::Extent3d {
                        width: 2048,
                        height: 2048,
                        depth_or_array_layers: 1,
                    },
                    bevy::render::render_resource::TextureDimension::D2,
                    &[0, 0, 0, 255],
                    bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                    bevy::asset::RenderAssetUsages::all(),
                )
            })
            .collect();
        assert_eq!(prepared.images[0].data.as_ref().unwrap().len(), FRAME_BYTES);
        let mut app = app(prepared);
        app.world_mut().resource_mut::<Epoch>().0 = 8;
        for remaining in [2, 1, 0] {
            app.update();
            assert_eq!(
                app.world().resource::<TestQueue>().0.images.len(),
                remaining
            );
            assert!(app.world().resource::<TestQueue>().0.retiring());
            assert!(app.world().resource::<Assets<Image>>().is_empty());
            assert!(app.world().resource::<Assets<Mesh>>().is_empty());
            assert!(app.world().resource::<TestQueue>().0.root.is_none());
            let mut meshes = app.world_mut().query_filtered::<Entity, With<Mesh3d>>();
            assert_eq!(meshes.iter(app.world()).count(), 0);
        }
        for _ in 0..100 {
            app.update();
            let queue = &app.world().resource::<TestQueue>().0;
            if queue.retiring_cpu_model == queue.models.len() && queue.instances.len() == 0 {
                break;
            }
        }
        let queue = &app.world().resource::<TestQueue>().0;
        assert_eq!(queue.retiring_cpu_model, queue.models.len());
        assert_eq!(queue.instances.len(), 0);
        assert!(!queue.published);
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

    #[test]
    fn tile_epoch_cardinality_receipt_and_unique_source_identity_refuse_before_admission() {
        use crate::ui::rectangles::Receipt;
        use std::sync::Arc;
        let fixture = || {
            let (mut prepared, _) = crate::fixture::prepare().unwrap();
            prepared.instances.truncate(1);
            let view = TileView {
                source: Arc::new(Receipt {
                    path: fallout_data::vfs::AssetPath::new(b"menus/authored.xml").unwrap(),
                    archive_sha256: "0".repeat(64),
                    payload_sha256: "1".repeat(64),
                }),
                node: 0,
                span: crate::ui::Span { start: 0, end: 10 },
                root_node: 0,
                epoch: 7,
            };
            (prepared, view)
        };
        let (prepared, _) = fixture();
        assert!(Queue::new_tiles(7, prepared, Vec::new()).is_err());
        let (prepared, mut view) = fixture();
        view.epoch = 8;
        assert!(Queue::new_tiles(7, prepared, vec![view]).is_err());
        let (prepared, mut view) = fixture();
        view.span.end = 0;
        assert!(Queue::new_tiles(7, prepared, vec![view]).is_err());
        let (prepared, mut view) = fixture();
        Arc::get_mut(&mut view.source).unwrap().archive_sha256 = "bad".into();
        assert!(Queue::new_tiles(7, prepared, vec![view]).is_err());
        let (mut prepared, view) = fixture();
        prepared.instances.push(scene::Instance {
            model: 0,
            transform: Transform::IDENTITY,
            key: None,
            visibility: Visibility::Inherited,
            canonical: None,
        });
        assert!(Queue::new_tiles(7, prepared, vec![view.clone(), view]).is_err());
    }
}
