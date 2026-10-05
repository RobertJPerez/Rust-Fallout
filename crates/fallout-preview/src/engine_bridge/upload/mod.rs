//! Initial immutable draw assets, observed in Bevy's actual render world.
//! Asset transfer completion does not prove a visible frame or shader readiness.
mod completion;

use super::{Phase, SceneLifetime};
use bevy::{
    asset::{Asset, AssetId},
    pbr::{Material, MaterialBindGroupAllocators, PreparedMaterial},
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        erased_render_asset::ErasedRenderAssets,
        mesh::{RenderMesh, RenderMeshBufferInfo, allocator::MeshAllocator},
        render_asset::RenderAssets,
        renderer::RenderQueue,
        texture::GpuImage,
    },
};
use completion::Completion;
use fallout_data::{
    resource_jobs::{JobError, JobResult},
    world::residency::{ResidentSources, Ticket},
};
use std::{
    any::TypeId,
    collections::BTreeSet,
    marker::PhantomData,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

const MAX_MESHES: usize = 16_384;
const MAX_MATERIALS: usize = 16_384;
const MAX_IMAGES: usize = 4096;

/// Every unique asset submitted for this scene. The consumer must keep their
/// contents immutable until acknowledgement and include all source textures.
/// This observer does not derive a model-to-asset mapping from caller handles.
pub struct UploadAssets<M: Material> {
    pub meshes: Vec<Handle<Mesh>>,
    pub materials: Vec<Handle<M>>,
    pub images: Vec<Handle<Image>>,
}

struct Watch<M: Material> {
    assets: UploadAssets<M>,
    source: Ticket,
    owner: Weak<ResidentSources>,
    scene_epoch: u64,
    completion: Arc<Completion>,
}

/// One pending initial scene upload. The main and render worlds share the same
/// bounded slot; no canonical state or source payload is copied into it.
#[derive(Resource)]
pub struct UploadMonitor<M: Material> {
    pending: Arc<Mutex<Option<Arc<Watch<M>>>>>,
    backend: Arc<AtomicU64>,
}
impl<M: Material> Clone for UploadMonitor<M> {
    fn clone(&self) -> Self {
        Self {
            pending: Arc::clone(&self.pending),
            backend: Arc::clone(&self.backend),
        }
    }
}
impl<M: Material> Default for UploadMonitor<M> {
    fn default() -> Self {
        Self {
            pending: Arc::new(Mutex::new(None)),
            backend: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Keep this with the draw queue until acknowledgement or cancellation. Drop
/// revokes it; the render observer releases its copy on the next bounded poll.
pub struct UploadTicket<M: Material> {
    watch: Arc<Watch<M>>,
}
impl<M: Material> Drop for UploadTicket<M> {
    fn drop(&mut self) {
        self.watch.completion.cancel();
    }
}
impl<M: Material> UploadTicket<M> {
    pub fn ready_for(&self, scene: &SceneLifetime, scene_epoch: u64) -> JobResult<bool> {
        scene.validate(scene_epoch)?;
        if scene.phase != Phase::Submitted || scene_epoch != self.watch.scene_epoch {
            return Err(JobError::Stale);
        }
        let models = scene.models.as_ref().ok_or(JobError::Closed)?;
        if !Weak::ptr_eq(&self.watch.owner, &Arc::downgrade(models)) {
            return Err(JobError::Invalid(
                "upload belongs to another source owner".into(),
            ));
        }
        self.watch.source.check()?;
        if !self.watch.completion.current() {
            return Err(JobError::Stale);
        }
        Ok(self.watch.completion.ready())
    }

    /// Join the actual acknowledgement with scene publication without retaining
    /// a cached readiness boolean across frames or borrowing the scene twice.
    pub fn publish_uploaded<T>(
        &self,
        scene: &mut SceneLifetime,
        scene_epoch: u64,
        publish: impl FnOnce() -> JobResult<T>,
    ) -> JobResult<Option<T>> {
        self.ready_for(scene, scene_epoch)?;
        scene.publish_uploaded(
            scene_epoch,
            || {
                self.watch.source.check()?;
                if !self.watch.completion.current() {
                    return Err(JobError::Stale);
                }
                Ok(self.watch.completion.ready())
            },
            publish,
        )
    }

    /// Call before mutating any watched asset or beginning draw retirement.
    pub fn cancel(&self) {
        self.watch.completion.cancel();
    }
}

fn validate_handles<A: Asset>(handles: &[Handle<A>], max: usize) -> JobResult<()> {
    if handles.len() > max {
        return Err(JobError::Invalid(
            "upload asset observation limit exceeded".into(),
        ));
    }
    let mut unique = BTreeSet::<AssetId<A>>::new();
    for handle in handles {
        if !handle.is_strong() || !unique.insert(handle.id()) {
            return Err(JobError::Invalid(
                "upload needs unique strong asset handles".into(),
            ));
        }
    }
    Ok(())
}

impl<M: Material> UploadMonitor<M> {
    pub fn watch(
        &self,
        scene: &SceneLifetime,
        scene_epoch: u64,
        assets: UploadAssets<M>,
    ) -> JobResult<UploadTicket<M>> {
        scene.validate(scene_epoch)?;
        if scene.phase != Phase::Submitted
            || assets.meshes.is_empty()
            || assets.materials.is_empty()
        {
            return Err(JobError::Invalid(
                "upload observation needs submitted nonempty draws".into(),
            ));
        }
        validate_handles(&assets.meshes, MAX_MESHES)?;
        validate_handles(&assets.materials, MAX_MATERIALS)?;
        validate_handles(&assets.images, MAX_IMAGES)?;
        let epoch = self.backend.load(Ordering::Acquire);
        if epoch == 0 {
            return Err(JobError::Invalid(
                "render upload observer is not initialized".into(),
            ));
        }
        let mut pending = self.pending.lock().map_err(|_| JobError::Closed)?;
        if pending.as_ref().is_some_and(|watch| {
            watch.completion.current()
                && watch.source.check().is_ok()
                && watch.owner.strong_count() != 0
        }) {
            return Err(JobError::Invalid(
                "an upload observation is already retained".into(),
            ));
        }
        let watch = Arc::new(Watch {
            assets,
            source: scene.ticket.clone(),
            owner: Arc::downgrade(scene.models.as_ref().ok_or(JobError::Closed)?),
            scene_epoch,
            completion: Arc::new(Completion::new(Arc::clone(&self.backend), epoch)),
        });
        *pending = Some(Arc::clone(&watch));
        Ok(UploadTicket { watch })
    }
}

/// Install after Bevy's renderer and the consumer's MaterialPlugin<M>. No new
/// loader or material pipeline is installed. Without RenderApp, watch refuses.
pub struct UploadPlugin<M: Material>(PhantomData<fn() -> M>);
impl<M: Material> Default for UploadPlugin<M> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<M: Material> Plugin for UploadPlugin<M> {
    fn build(&self, app: &mut App) {
        app.init_resource::<UploadMonitor<M>>();
        let monitor = app.world().resource::<UploadMonitor<M>>().clone();
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(monitor)
                .add_systems(RenderStartup, reset_backend::<M>)
                .add_systems(Render, observe::<M>.in_set(RenderSystems::Cleanup));
        }
    }
}

fn reset_backend<M: Material>(monitor: Res<UploadMonitor<M>>) {
    // RenderStartup also runs on device recovery. Old callbacks and receipts
    // can never certify the new device, even when numeric scene epochs match.
    if monitor
        .backend
        .try_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
            epoch.checked_add(1)
        })
        .is_err()
    {
        monitor.backend.store(0, Ordering::Release);
    }
    if let Ok(mut pending) = monitor.pending.lock()
        && let Some(watch) = pending.take()
    {
        watch.completion.cancel();
    }
}

fn observe<M: Material>(
    monitor: Res<UploadMonitor<M>>,
    meshes: Option<Res<RenderAssets<RenderMesh>>>,
    allocator: Option<Res<MeshAllocator>>,
    images: Option<Res<RenderAssets<GpuImage>>>,
    materials: Option<Res<ErasedRenderAssets<PreparedMaterial>>>,
    bindings: Option<Res<MaterialBindGroupAllocators>>,
    queue: Option<Res<RenderQueue>>,
) {
    let watch = {
        let Ok(mut pending) = monitor.pending.lock() else {
            return;
        };
        if pending.as_ref().is_some_and(|watch| {
            !watch.completion.current()
                || watch.source.check().is_err()
                || watch.owner.strong_count() == 0
        }) {
            pending.take();
        }
        pending.as_ref().map(Arc::clone)
    };
    let Some(watch) = watch else { return };
    let (Some(meshes), Some(allocator), Some(images), Some(materials), Some(bindings), Some(queue)) =
        (meshes, allocator, images, materials, bindings, queue)
    else {
        return;
    };

    for handle in &watch.assets.meshes {
        let id = handle.id();
        let Some(mesh) = meshes.get(id) else { return };
        let Some(vertices) = allocator.mesh_vertex_slice(&id) else {
            return;
        };
        if mesh.vertex_count == 0 || vertices.range.len() < mesh.vertex_count as usize {
            return;
        }
        if let RenderMeshBufferInfo::Indexed { count, .. } = mesh.buffer_info {
            let Some(indices) = allocator.mesh_index_slice(&id) else {
                return;
            };
            if count == 0 || indices.range.len() < count as usize {
                return;
            }
        }
    }
    for handle in &watch.assets.images {
        let Some(image) = images.get(handle.id()) else {
            return;
        };
        let size = image.texture_descriptor.size;
        if !image.had_data || size.width == 0 || size.height == 0 || size.depth_or_array_layers == 0
        {
            return;
        }
    }
    let Some(material_allocator) = bindings.get(&TypeId::of::<M>()) else {
        return;
    };
    for handle in &watch.assets.materials {
        let Some(material) = materials.get(handle.id()) else {
            return;
        };
        let Some(slab) = material_allocator.get(material.binding.group) else {
            return;
        };
        if slab.bind_group().is_none() {
            return;
        }
    }
    if watch.source.check().is_err() {
        watch.completion.cancel();
        return;
    }
    if let Some(callback) = watch.completion.callback() {
        // Flush any pending write_buffer/write_texture transfers even if the
        // scene is still hidden. The callback observes this real submission;
        // it performs no World, visibility, asset or source-owner mutation.
        queue.submit([]);
        queue.on_submitted_work_done(callback);
    }
}
