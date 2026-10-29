//! The game-master blocks' editors: `StructureBlockEditScreen`,
//! `JigsawBlockEditScreen`, `TestBlockEditScreen` and
//! `TestInstanceBlockEditScreen`. Using one in creative with operator
//! rights opens its editor, filled from the block's data; Done (or an
//! action button) sends the block's packet.

use std::collections::HashMap;
use std::sync::Mutex;

use azalea_core::position::BlockPos;
use simdnbt::owned::NbtCompound;

use crate::gui::painter::Painter;
use crate::gui::widgets::{self, Button, TextBox, WIDGET_HEIGHT};
use crate::gui::{Screen, ScreenCtx};

/// The raw data of the game-master blocks the client knows, by position.
static TAGS: Mutex<Option<HashMap<[i32; 3], NbtCompound>>> = Mutex::new(None);

pub fn store_tag(pos: [i32; 3], tag: &NbtCompound) {
    TAGS.lock().unwrap().get_or_insert_with(HashMap::new).insert(pos, tag.clone());
}

fn tag_at(pos: BlockPos) -> NbtCompound {
    TAGS.lock()
        .unwrap()
        .as_ref()
        .and_then(|t| t.get(&[pos.x, pos.y, pos.z]).cloned())
        .unwrap_or_default()
}

/// Which editor, for which block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpBlock {
    Structure,
    Jigsaw,
    Test,
    TestInstance,
}

impl OpBlock {
    pub fn of(id: &str) -> Option<OpBlock> {
        Some(match id {
            "structure_block" => OpBlock::Structure,
            "jigsaw" => OpBlock::Jigsaw,
            "test_block" => OpBlock::Test,
            "test_instance_block" => OpBlock::TestInstance,
            _ => return None,
        })
    }
}

/// A request to the server, sent by the tick.
#[derive(Clone, Debug)]
pub enum OpBlockSend {
    Structure(azalea_protocol::packets::game::s_set_structure_block::ServerboundSetStructureBlock),
    Jigsaw(azalea_protocol::packets::game::s_set_jigsaw_block::ServerboundSetJigsawBlock),
    JigsawGenerate(azalea_protocol::packets::game::s_jigsaw_generate::ServerboundJigsawGenerate),
    Test(azalea_protocol::packets::game::s_set_test_block::ServerboundSetTestBlock),
}

static OUTBOX: Mutex<Vec<OpBlockSend>> = Mutex::new(Vec::new());

pub fn take_sends() -> Vec<OpBlockSend> {
    std::mem::take(&mut *OUTBOX.lock().unwrap())
}

fn send(packet: OpBlockSend) {
    OUTBOX.lock().unwrap().push(packet);
}

/// A labelled text field.
struct Field {
    label: &'static str,
    input: TextBox,
}

impl Field {
    fn new(label: &'static str, value: &str, max: usize) -> Field {
        let mut input = TextBox::bordered(max, "");
        input.set_text(value);
        Field { label, input }
    }
}

pub struct OpBlockState {
    pub kind: OpBlock,
    pos: BlockPos,
    fields: Vec<Field>,
    /// Cycle buttons' choices, and the picked one.
    cycles: Vec<(&'static str, Vec<&'static str>, usize)>,
    toggles: Vec<(&'static str, bool)>,
    levels: i32,
    keep_jigsaws: bool,
}

impl Default for OpBlockState {
    fn default() -> Self {
        OpBlockState {
            kind: OpBlock::Structure,
            pos: BlockPos::default(),
            fields: Vec::new(),
            cycles: Vec::new(),
            toggles: Vec::new(),
            levels: 0,
            keep_jigsaws: true,
        }
    }
}

const STRUCTURE_MODES: [&str; 4] = ["save", "load", "corner", "data"];
const MIRRORS: [&str; 3] = ["none", "left_right", "front_back"];
const ROTATIONS: [&str; 4] = ["none", "clockwise_90", "180", "counterclockwise_90"];
const JOINTS: [&str; 2] = ["rollable", "aligned"];
const TEST_MODES: [&str; 4] = ["start", "log", "fail", "accept"];

fn string(tag: &NbtCompound, key: &str, default: &str) -> String {
    tag.string(key).map_or_else(|| default.to_owned(), |s| s.to_string())
}

fn int(tag: &NbtCompound, key: &str, default: i32) -> i32 {
    tag.int(key).unwrap_or(default)
}

fn flag(tag: &NbtCompound, key: &str) -> bool {
    tag.byte(key).is_some_and(|b| b != 0)
}

fn index_of(options: &[&str], value: &str) -> usize {
    options.iter().position(|o| o.eq_ignore_ascii_case(value)).unwrap_or(0)
}

impl OpBlockState {
    pub fn open(&mut self, kind: OpBlock, pos: BlockPos) {
        let tag = tag_at(pos);
        *self = OpBlockState { kind, pos, ..Default::default() };
        match kind {
            OpBlock::Structure => {
                self.fields = vec![
                    Field::new("Structure Name", &string(&tag, "name", ""), 128),
                    Field::new("Relative X", &int(&tag, "posX", 0).to_string(), 15),
                    Field::new("Relative Y", &int(&tag, "posY", 1).to_string(), 15),
                    Field::new("Relative Z", &int(&tag, "posZ", 0).to_string(), 15),
                    Field::new("Size X", &int(&tag, "sizeX", 0).to_string(), 15),
                    Field::new("Size Y", &int(&tag, "sizeY", 0).to_string(), 15),
                    Field::new("Size Z", &int(&tag, "sizeZ", 0).to_string(), 15),
                    Field::new("Integrity", &format!("{}", tag.float("integrity").unwrap_or(1.0)), 15),
                    Field::new("Seed", &tag.long("seed").unwrap_or(0).to_string(), 31),
                    Field::new("Custom Data Tag Name", &string(&tag, "metadata", ""), 128),
                ];
                self.cycles = vec![
                    ("Mode", STRUCTURE_MODES.to_vec(), index_of(&STRUCTURE_MODES, &string(&tag, "mode", "data"))),
                    ("Mirror", MIRRORS.to_vec(), index_of(&MIRRORS, &string(&tag, "mirror", "none"))),
                    ("Rotation", ROTATIONS.to_vec(), index_of(&ROTATIONS, &string(&tag, "rotation", "none"))),
                ];
                self.toggles = vec![
                    ("Include Entities", !flag(&tag, "ignoreEntities")),
                    ("Show Air", flag(&tag, "showair")),
                    ("Show Bounding Box", tag.byte("showboundingbox").is_none_or(|b| b != 0)),
                    ("Strict Placement", flag(&tag, "strict")),
                ];
            }
            OpBlock::Jigsaw => {
                self.fields = vec![
                    Field::new("Name", &string(&tag, "name", "minecraft:empty"), 128),
                    Field::new("Target Name", &string(&tag, "target", "minecraft:empty"), 128),
                    Field::new("Target Pool", &string(&tag, "pool", "minecraft:empty"), 128),
                    Field::new("Turns into", &string(&tag, "final_state", "minecraft:air"), 256),
                    Field::new("Selection Priority", &int(&tag, "selection_priority", 0).to_string(), 10),
                    Field::new("Placement Priority", &int(&tag, "placement_priority", 0).to_string(), 10),
                ];
                self.cycles = vec![("Joint Type", JOINTS.to_vec(), index_of(&JOINTS, &string(&tag, "joint", "rollable")))];
            }
            OpBlock::Test => {
                self.fields = vec![Field::new("Message", &string(&tag, "message", ""), 128)];
                self.cycles = vec![("Mode", TEST_MODES.to_vec(), index_of(&TEST_MODES, &string(&tag, "mode", "start")))];
            }
            OpBlock::TestInstance => {
                let data = tag.compound("data").cloned().unwrap_or_default();
                let size = data.int_array("size").map(|a| a.to_vec()).unwrap_or_default();
                let axis = |i: usize| size.get(i).copied().unwrap_or(0).to_string();
                self.fields = vec![
                    Field::new("Test", &string(&data, "test", ""), 256),
                    Field::new("Size X", &axis(0), 15),
                    Field::new("Size Y", &axis(1), 15),
                    Field::new("Size Z", &axis(2), 15),
                ];
                self.cycles = vec![("Rotation", ROTATIONS.to_vec(), index_of(&ROTATIONS, &string(&data, "rotation", "none")))];
                self.toggles = vec![("Include Entities", !flag(&data, "ignore_entities"))];
            }
        }
    }

    fn text(&self, i: usize) -> &str {
        self.fields.get(i).map_or("", |f| f.input.text.as_str())
    }

    fn number(&self, i: usize) -> i32 {
        self.text(i).trim().parse().unwrap_or(0)
    }

    fn cycle(&self, i: usize) -> usize {
        self.cycles.get(i).map_or(0, |c| c.2)
    }

    fn toggle(&self, i: usize) -> bool {
        self.toggles.get(i).is_some_and(|t| t.1)
    }

    fn structure(&self, update: u8) -> OpBlockSend {
        use azalea_protocol::packets::game::s_set_structure_block::*;
        let byte = |i: usize| self.number(i).clamp(-48, 48) as i8 as u8;
        let size = |i: usize| self.number(i).clamp(0, 48) as u8;
        OpBlockSend::Structure(ServerboundSetStructureBlock {
            pos: self.pos,
            update_type: match update {
                1 => UpdateType::SaveArea,
                2 => UpdateType::LoadArea,
                3 => UpdateType::ScanArea,
                _ => UpdateType::UpdateData,
            },
            mode: match self.cycle(0) {
                0 => StructureMode::Save,
                1 => StructureMode::Load,
                2 => StructureMode::Corner,
                _ => StructureMode::Data,
            },
            name: self.text(0).to_owned(),
            offset: BytePosition { x: byte(1), y: byte(2), z: byte(3) },
            size: BytePosition { x: size(4), y: size(5), z: size(6) },
            mirror: match self.cycle(1) {
                1 => Mirror::LeftRight,
                2 => Mirror::FrontBack,
                _ => Mirror::None,
            },
            rotation: match self.cycle(2) {
                1 => Rotation::Clockwise90,
                2 => Rotation::Clockwise180,
                3 => Rotation::Counterclockwise90,
                _ => Rotation::None,
            },
            data: self.text(9).to_owned(),
            integrity: self.text(7).trim().parse::<f32>().unwrap_or(1.0).clamp(0.0, 1.0),
            seed: self.text(8).trim().parse::<i64>().unwrap_or(0) as u64,
            flags: Flags {
                ignore_entities: !self.toggle(0),
                show_air: self.toggle(1),
                show_bounding_box: self.toggle(2),
            },
        })
    }

    fn jigsaw(&self) -> OpBlockSend {
        use azalea_protocol::packets::game::s_set_jigsaw_block::ServerboundSetJigsawBlock;
        let id = |i: usize| {
            let text = self.text(i).trim();
            text.parse().unwrap_or_else(|_| "minecraft:empty".parse().unwrap())
        };
        OpBlockSend::Jigsaw(ServerboundSetJigsawBlock {
            pos: self.pos,
            name: id(0),
            target: id(1),
            pool: id(2),
            final_state: self.text(3).to_owned(),
            joint: JOINTS[self.cycle(0)].to_owned(),
            selection_priority: self.number(4),
            placement_priority: self.number(5),
        })
    }

    fn test(&self) -> OpBlockSend {
        use azalea_protocol::packets::game::s_set_test_block::{ServerboundSetTestBlock, TestBlockMode};
        OpBlockSend::Test(ServerboundSetTestBlock {
            position: self.pos,
            mode: match self.cycle(0) {
                1 => TestBlockMode::Log,
                2 => TestBlockMode::Fail,
                3 => TestBlockMode::Accept,
                _ => TestBlockMode::Start,
            },
            message: self.text(0).to_owned(),
        })
    }
}

fn title(kind: OpBlock) -> &'static str {
    match kind {
        OpBlock::Structure => "Structure Block",
        OpBlock::Jigsaw => "Jigsaw Block",
        OpBlock::Test => "Test Block",
        OpBlock::TestInstance => "Test Instance Block",
    }
}

pub fn draw(p: &mut Painter, state: &mut OpBlockState, ctx: &ScreenCtx) -> Option<Screen> {
    crate::gui::menu::background(p, ctx.vw, ctx.vh);
    widgets::draw_title(p, ctx.vw, 10.0, title(state.kind));
    let cx = (ctx.vw / 2.0).floor();
    // Fields two to a row (the first full width), then cycles, toggles and
    // the actions.
    let col_w = 150.0;
    let full_w = 304.0;
    let left = cx - 152.0;
    let mut y = 30.0;
    let frame = p.frame;
    for (i, field) in state.fields.iter_mut().enumerate() {
        let wide = i == 0 || field.input.max_len > 64;
        let (x, w) = if wide { (left, full_w) } else if (i % 2 == 1) == (state.kind != OpBlock::TestInstance) { (left, col_w) } else { (left + 154.0, col_w) };
        p.text_str(field.label, x + 1.0, y, 0xA0A0A0, true);
        field.input.handle_mouse(ctx.input, &p.atlas.font, x, y + 10.0, w, WIDGET_HEIGHT);
        if field.input.focused {
            field.input.handle_input(ctx.input);
        }
        field.input.draw(p, x, y + 10.0, w, WIDGET_HEIGHT, frame);
        if wide || x > left {
            y += 34.0;
        }
    }
    if state.fields.len() % 2 == 0 && state.kind != OpBlock::Jigsaw {
        y += 0.0;
    }
    let mut x = left;
    for (label, options, picked) in state.cycles.iter_mut() {
        let caption = format!("{label}: {}", options[*picked]);
        if Button::new(x, y, 100.0, WIDGET_HEIGHT, &caption).draw(p, ctx) {
            *picked = (*picked + 1) % options.len();
        }
        x += 102.0;
    }
    if !state.cycles.is_empty() {
        y += 24.0;
    }
    let mut x = left;
    for (label, on) in state.toggles.iter_mut() {
        let caption = format!("{label}: {}", if *on { "ON" } else { "OFF" });
        if Button::new(x, y, 150.0, WIDGET_HEIGHT, &caption).draw(p, ctx) {
            *on = !*on;
        }
        if x > left {
            x = left;
            y += 24.0;
        } else {
            x = left + 154.0;
        }
    }
    if x > left {
        y += 24.0;
    }

    // The block's actions.
    let mut close = false;
    match state.kind {
        OpBlock::Structure => {
            let mode = state.cycle(0);
            let action = match mode {
                0 => Some(("Save", 1u8)),
                1 => Some(("Load", 2)),
                2 => None,
                _ => None,
            };
            if let Some((label, update)) = action
                && Button::new(left, y, 100.0, WIDGET_HEIGHT, label).draw(p, ctx)
            {
                send(state.structure(update));
                close = true;
            }
            if mode == 0 && Button::new(left + 102.0, y, 100.0, WIDGET_HEIGHT, "Detect").draw(p, ctx) {
                send(state.structure(3));
            }
        }
        OpBlock::Jigsaw => {
            let label = format!("Levels: {}", state.levels);
            if Button::new(left, y, 100.0, WIDGET_HEIGHT, &label).draw(p, ctx) {
                state.levels = (state.levels + 1) % 21;
            }
            let keep = format!("Keep Jigsaws: {}", if state.keep_jigsaws { "ON" } else { "OFF" });
            if Button::new(left + 102.0, y, 100.0, WIDGET_HEIGHT, &keep).draw(p, ctx) {
                state.keep_jigsaws = !state.keep_jigsaws;
            }
            if Button::new(left + 204.0, y, 100.0, WIDGET_HEIGHT, "Generate").draw(p, ctx) {
                send(state.jigsaw());
                send(OpBlockSend::JigsawGenerate(
                    azalea_protocol::packets::game::s_jigsaw_generate::ServerboundJigsawGenerate {
                        pos: state.pos,
                        levels: state.levels as u32,
                        keep_jigsaws: state.keep_jigsaws,
                    },
                ));
                close = true;
            }
        }
        OpBlock::Test => {}
        // azalea's test instance packet carries the test's type, not its
        // id, so sending it would clear the block's test: shown, not sent.
        OpBlock::TestInstance => {
            p.text_str("Editing test instances isn't supported yet", left, y + 6.0, 0xFF5555, true);
        }
    }

    let bottom = ctx.vh - 28.0;
    if Button::new(cx - 154.0, bottom, 150.0, WIDGET_HEIGHT, "Done").draw(p, ctx) {
        send(match state.kind {
            OpBlock::Structure => state.structure(0),
            OpBlock::Jigsaw => state.jigsaw(),
            OpBlock::Test => state.test(),
            OpBlock::TestInstance => {
                return Some(Screen::None);
            }
        });
        close = true;
    }
    if Button::new(cx + 4.0, bottom, 150.0, WIDGET_HEIGHT, "Cancel").draw(p, ctx) || ctx.input.escape {
        close = true;
    }
    close.then_some(Screen::None)
}
