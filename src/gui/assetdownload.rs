use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;

use crate::client::assets::{Stage, download};
use crate::gui::painter::Painter;
use crate::gui::{GuiState, Screen, ScreenCtx};
use crate::{log_info, log_warn};

const PROMPT: &str = "The Mojang assets must be downloaded";
const TOP_UP_PROMPT: &str = "Some Mojang assets are missing";

static TOP_UP: AtomicBool = AtomicBool::new(false);

pub(crate) fn check_missing(state: &mut GuiState) -> bool {
    if !crate::client::assets::check_missing() {
        return false;
    }
    TOP_UP.store(true, Ordering::Relaxed);
    state.nav = Some(Screen::AssetDownload);
    true
}

fn leave_label() -> &'static str {
    if TOP_UP.load(Ordering::Relaxed) {
        "Back"
    } else {
        "Exit"
    }
}

fn leave(state: &mut GuiState) {
    if TOP_UP.swap(false, Ordering::Relaxed) {
        state.nav = Some(Screen::ManageStorage);
    } else {
        state.quit = true;
    }
}

const BUTTON_W: f32 = 100.0;
const BUTTON_H: f32 = 20.0;
const BUTTON_GAP: f32 = 8.0;

const BAR_W: f32 = 220.0;
const BAR_H: f32 = 10.0;

const LINE: f32 = 12.0;

const MB: f64 = 1_048_576.0;

pub fn draw(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    p.gradient_v(0.0, 0.0, ctx.vw, ctx.vh, 0xFF1B1B21, 0xFF0C0C10);

    let cx = (ctx.vw * 0.5).floor();
    let mid = (ctx.vh * 0.5).floor();
    let dl = download();
    let stage = dl.stage();

    let top_up = TOP_UP.load(Ordering::Relaxed);
    let prompt = if top_up { TOP_UP_PROMPT } else { PROMPT };
    centered(p, prompt, cx, mid - 46.0, 0xFFFFFF);

    match stage {
        Stage::Idle | Stage::Cancelled => {
            if stage == Stage::Cancelled {
                centered(p, "Download cancelled", cx, mid - 46.0 + LINE, 0xFFAA55);
            }
            let row = mid + 6.0;
            let left = (cx - BUTTON_W - BUTTON_GAP * 0.5).floor();
            let right = (cx + BUTTON_GAP * 0.5).floor();
            if button(p, ctx, left, row, "Continue") {
                dl.start(crate::ASSET_VERSION.to_string());
            }
            if button(p, ctx, right, row, leave_label()) {
                leave(state);
            }
        }

        Stage::Asking
        | Stage::Downloading
        | Stage::Extracting
        | Stage::Fetching
        | Stage::Sounds => {
            let (done, total) = if stage == Stage::Sounds {
                dl.sound_progress()
            } else {
                dl.progress()
            };
            let frac = if total > 0 {
                (done as f32 / total as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            bar(p, ctx, mid - 14.0, frac);

            let percent = format!("{}%", (frac * 100.0) as u32);
            centered(p, &percent, cx, mid + 2.0, 0xFFFFFF);

            let detail = match stage {
                Stage::Asking => "Contacting Mojang".to_string(),
                Stage::Downloading | Stage::Fetching => megabytes(done, total),
                Stage::Sounds => format!("Sounds {}", megabytes(done, total)),
                _ => "Unpacking the assets".to_string(),
            };
            centered(p, &detail, cx, mid + 2.0 + LINE, 0xA0A0A8);

            let sounds = dl.sounds_running();
            if sounds && stage != Stage::Sounds {
                let (done, total) = dl.sound_progress();
                let line = format!("Sounds {}", megabytes(done, total));
                centered(p, &line, cx, mid + 2.0 + LINE * 2.0, 0x70707A);
            }

            let row = mid + 40.0;
            if sounds {
                let left = (cx - BUTTON_W - BUTTON_GAP * 0.5).floor();
                let right = (cx + BUTTON_GAP * 0.5).floor();
                if button(p, ctx, left, row, "Skip audio") {
                    dl.skip_sounds();
                }
                if button(p, ctx, right, row, "Cancel") {
                    dl.cancel();
                }
            } else if button(p, ctx, (cx - BUTTON_W * 0.5).floor(), row, "Cancel") {
                dl.cancel();
            }
        }

        Stage::Done if top_up => {
            crate::client::assets::adopt_sounds();
            leave(state);
        }
        Stage::Done => {
            prepare();
            centered(p, "The assets are ready", cx, mid - 14.0, 0x7FDF7F);
            centered(p, "Preparing the atlases", cx, mid - 14.0 + LINE, 0xA0A0A8);
        }

        Stage::Failed => {
            centered(p, dl.error(), cx, mid - 14.0, 0xFF5555);
            centered(p, "The log says more", cx, mid - 14.0 + LINE, 0xA0A0A8);
            let row = mid + 20.0;
            let left = (cx - BUTTON_W - BUTTON_GAP * 0.5).floor();
            let right = (cx + BUTTON_GAP * 0.5).floor();
            if button(p, ctx, left, row, "Try again") {
                dl.start(crate::ASSET_VERSION.to_string());
            }
            if button(p, ctx, right, row, leave_label()) {
                leave(state);
            }
        }
    }
}

fn megabytes(done: u64, total: u64) -> String {
    format!("{:.1} MB of {:.1} MB", done as f64 / MB, total as f64 / MB)
}

fn centered(p: &mut Painter, s: &str, cx: f32, y: f32, rgb: u32) {
    let w = p.atlas.font.width_str(s);
    p.text_str(s, (cx - w * 0.5).floor(), y, rgb, false);
}

fn button(p: &mut Painter, ctx: &ScreenCtx, x: f32, y: f32, label: &str) -> bool {
    let hovered = ctx.hovering(x, y, BUTTON_W, BUTTON_H);
    let (face, edge) = if hovered {
        (0xFF39_3944, 0xFFFF_FFFF)
    } else {
        (0xFF22_2229, 0xFF6E_6E7A)
    };
    p.fill(x, y, BUTTON_W, BUTTON_H, face);
    p.outline(x, y, BUTTON_W, BUTTON_H, edge);
    let w = p.atlas.font.width_str(label);
    p.text_str(
        label,
        (x + (BUTTON_W - w) * 0.5).floor(),
        (y + (BUTTON_H - 8.0) * 0.5).floor(),
        0xFFFFFF,
        false,
    );
    hovered && ctx.input.left_click
}

fn bar(p: &mut Painter, ctx: &ScreenCtx, y: f32, frac: f32) {
    let w = BAR_W.min(ctx.vw - 40.0).max(60.0).floor();
    let x = ((ctx.vw - w) * 0.5).floor();
    p.fill(x, y, w, BAR_H, 0xFF0A0A0C);
    let filled = ((w - 2.0) * frac).floor();
    if filled >= 1.0 {
        p.fill(x + 1.0, y + 1.0, filled, BAR_H - 2.0, 0xFF4C_C46A);
    }
    p.outline(x, y, w, BAR_H, 0xFF6E_6E7A);
}

struct Prepared {
    block_atlas: Image,
    block_tiles: std::collections::HashMap<String, u32>,
    gui_atlas: crate::gui::atlas::GuiAtlas,
}

static PREPARED: Mutex<Option<Prepared>> = Mutex::new(None);

static PREPARING: AtomicBool = AtomicBool::new(false);

fn prepare() {
    if PREPARING.swap(true, Ordering::Relaxed) {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    build();
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::Builder::new()
        .name("asset-prepare".into())
        .spawn(build)
        .ok();
}

fn build() {
    if !crate::client::assets::mount() {
        PREPARING.store(false, Ordering::Relaxed);
        download().report_failure(
            "The assets could not be read",
            "the set that just installed will not mount",
        );
        return;
    }
    let textures_dir = crate::textures_dir().to_string_lossy().into_owned();
    let textures = crate::renderer::Textures::blocks(&textures_dir);
    let (block_atlas, block_tiles) = crate::renderer::build_block_atlas(&textures, &textures_dir);
    let gui_atlas = crate::gui::atlas::build_gui_atlas(&crate::assets_root());
    *PREPARED.lock().unwrap() = Some(Prepared {
        block_atlas,
        block_tiles,
        gui_atlas,
    });
    log_info!("assets", "atlases rebuilt from the installed set");
    #[cfg(target_arch = "wasm32")]
    if let Some(archive) = crate::client::assets::take_installed() {
        crate::client::mesh_worker::spawn(&archive);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn adopt_downloaded_assets(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    block: Option<Res<crate::renderer::systems::BlockAtlasHandle>>,
    chunk: Option<Res<crate::renderer::systems::ChunkMaterial>>,
    water: Option<Res<crate::renderer::systems::WaterMaterial>>,
    gui: Option<ResMut<crate::gui::render::GuiAssets>>,
    break_overlay: Option<Res<crate::renderer::overlays::BreakOverlay>>,
    after: Res<crate::gui::render::AfterDownloadScreen>,
    mut state: ResMut<GuiState>,
) {
    let Some(prepared) = PREPARED.lock().unwrap().take() else {
        return;
    };
    let (Some(block), Some(chunk), Some(water), Some(mut gui)) = (block, chunk, water, gui) else {
        *PREPARED.lock().unwrap() = Some(prepared);
        return;
    };

    let rows = crate::renderer::atlas_rows(&prepared.block_atlas);
    crate::TEXTURE_MAP.set(prepared.block_tiles.clone()).ok();
    crate::ATLAS_ROWS.set(rows).ok();
    commands.insert_resource(crate::renderer::systems::BlockTileMap(prepared.block_tiles));

    if let Err(e) = images.insert(&block.0, prepared.block_atlas) {
        log_warn!("assets", "the block atlas could not be replaced: {e}");
    }
    standard.get_mut(&chunk.0);
    standard.get_mut(&water.0);
    if let Some(overlay) = break_overlay.as_ref() {
        standard.get_mut(&overlay.material);
    }

    let (image, unihex) = crate::gui::render::atlas_textures(&prepared.gui_atlas);
    if let Err(e) = images.insert(&gui.image, image) {
        log_warn!("assets", "the gui atlas could not be replaced: {e}");
    }
    if let Err(e) = images.insert(&gui.unihex_image, unihex) {
        log_warn!("assets", "the unifont texture could not be replaced: {e}");
    }
    gui.atlas = std::sync::Arc::new(prepared.gui_atlas);

    commands.insert_resource(crate::renderer::systems::AssetsReady);
    state.screen = after.0;
    log_info!("assets", "the downloaded set is live");
}
