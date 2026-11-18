//! Compiles the selected shader pack off the main thread and installs it as
//! the `CompiledPack` resource; publishes its block ids for meshing.

use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::Task;

use crate::client::dimension::Dimension;
use crate::renderer::terrain_pool::TerrainTier;
use crate::shaderpack::compile::{self, Frame};
use crate::shaderpack::install::{Installed, install, report};

#[derive(Resource)]
pub(crate) struct CompiledPack {
    name: String,
    revision: u64,
    dimension: Dimension,
    pub(crate) installed: Arc<Installed>,
}

pub(crate) struct Pending {
    name: String,
    revision: u64,
    dimension: Dimension,
    task: Task<Result<Frame, String>>,
}

pub(crate) fn sync_compiled_pack(
    mut commands: Commands,
    gui: Res<crate::gui::GuiState>,
    tier: Option<Res<TerrainTier>>,
    compiled: Option<Res<CompiledPack>>,
    mut failed: Local<Option<(String, u64, Dimension)>>,
    mut generation: Local<u32>,
    mut pending: Local<Option<Pending>>,
) {
    let Some(tier) = tier else { return };
    let loaded = gui.shaderpacks.loaded.as_ref();
    let wanted = loaded.map(|l| l.name.clone());
    let revision = gui.shaderpacks.revision();
    let editing = gui.screen == crate::gui::Screen::ShaderOptions;
    let direct = !tier.indirect;
    let dimension = crate::client::dimension::current();

    if let Some(running) = pending.as_mut() {
        let current = Some(&running.name) == wanted.as_ref()
            && (editing || running.revision == revision)
            && running.dimension == dimension;
        if !current {
            *pending = None;
        } else {
            let Some(result) = bevy::tasks::futures::check_ready(&mut running.task) else {
                return;
            };
            let Pending {
                name,
                revision,
                dimension,
                ..
            } = pending.take().expect("matched above");
            match result {
                Ok(mut frame) => {
                    let notes = std::mem::take(&mut frame.notes);
                    let installed = install(&name, *generation, direct, frame);
                    report(&name, &installed, &notes);
                    *failed = None;
                    commands.insert_resource(CompiledPack {
                        name,
                        revision,
                        dimension,
                        installed: Arc::new(installed),
                    });
                }
                Err(e) => {
                    crate::log_warn!("shaders", "{name} did not compile: {e}");
                    *failed = Some((name, revision, dimension));
                    commands.remove_resource::<CompiledPack>();
                }
            }
            return;
        }
    }

    let stale = match compiled.as_deref() {
        None => wanted.is_some(),
        Some(current) => {
            Some(&current.name) != wanted.as_ref()
                || (!editing && current.revision != revision)
                || current.dimension != dimension
        }
    };
    if !stale {
        return;
    }
    let (Some(name), Some(loaded)) = (wanted, loaded) else {
        commands.remove_resource::<CompiledPack>();
        *failed = None;
        return;
    };
    if *failed == Some((name.clone(), revision, dimension)) {
        return;
    }

    *generation = generation.wrapping_add(1).max(1);
    let values = loaded.values().clone();
    let task_name = name.clone();
    let task = bevy::tasks::AsyncComputeTaskPool::get()
        .spawn(async move { compile::compile_frame(&task_name, direct, Some(values), dimension) });
    *pending = Some(Pending {
        name,
        revision,
        dimension,
        task,
    });
}

static REJECTED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub(crate) fn reject(generation: u32) {
    REJECTED.store(generation, std::sync::atomic::Ordering::Release);
}

pub(crate) fn sync_pack_vertices(
    compiled: Option<Res<CompiledPack>>,
    shared: Res<crate::renderer::systems::Shared>,
    mut published: Local<Option<Arc<crate::shaderpack::packvertex::BlockIds>>>,
) {
    let rejected = REJECTED.load(std::sync::atomic::Ordering::Acquire);
    let live = compiled
        .as_deref()
        .filter(|c| c.installed.generation != rejected);
    let player_shadow = live.is_some_and(|c| {
        let casters = c.installed.shadow_casters;
        c.installed.entities[compile::EntityKind::ShadowCaster.index()].is_some()
            && (casters.player || casters.entities)
    });
    crate::shaderpack::packvertex::set_player_shadow(player_shadow);
    let wanted = live.and_then(|c| c.installed.block_ids.clone());
    let remesh = match (published.as_ref(), wanted.as_ref()) {
        (None, None) => return,
        (Some(old), Some(new)) if Arc::ptr_eq(old, new) => return,
        (Some(old), Some(new)) => old != new,
        _ => true,
    };
    crate::shaderpack::packvertex::set_current(wanted.clone());
    *published = wanted;
    if remesh {
        let _ = &shared;
        crate::client::bus::request(crate::client::bus::ClientRequest::ReloadChunks);
    }
}
