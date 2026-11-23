//! Create 6.0.10's client renderers, ported to the Torch mod API
//! (`docs/porting/AGENT_GUIDE.md`). Round 1: kinetic parts (shafts, cogs,
//! gearboxes, gearshifts, motors, water wheels, crushing wheels) and tank
//! fluid. Round 2: belts, pumps, hand cranks, speed controllers, bearings.
//! Round 3: pipe attachments. What's done and skipped: `PORTED.md`.
//!
//! Create's code is MIT; its assets are All Rights Reserved, so this port
//! names models and sprites but carries none.

mod belt;
mod dir;
mod kinetic;
mod pipe;
mod tank;

use torch_mod_api::{BlockEntityView, Plugin, Registration, Rig, export_plugin};

/// Block entity types whose block model Create leaves out of the terrain:
/// the bracketed kinetic blocks (`BracketedKineticBlockModel` returns no
/// quads) and `ENTITYBLOCK_ANIMATED` ones. Their renderer draws the block.
const REPLACES_BLOCK_MODEL: &[&str] = &[
    "create:simple_kinetic",
    "create:turntable",
    "create:gantry_shaft",
    "create:powered_shaft",
    "create:crushing_wheel",
    "create:large_water_wheel",
    "create:hand_crank",
];

const HANDLED: &[&str] = &[
    "create:simple_kinetic",
    "create:encased_shaft",
    "create:adjustable_chain_gearshift",
    "create:encased_cogwheel",
    "create:encased_large_cogwheel",
    "create:gearbox",
    "create:clutch",
    "create:gearshift",
    "create:sequenced_gearshift",
    "create:motor",
    "create:turntable",
    "create:gantry_shaft",
    "create:powered_shaft",
    "create:crushing_wheel",
    "create:water_wheel",
    "create:large_water_wheel",
    "create:fluid_tank",
    "create:creative_fluid_tank",
    "create:belt",
    "create:mechanical_pump",
    "create:hand_crank",
    "create:rotation_speed_controller",
    "create:mechanical_bearing",
    "create:windmill_bearing",
    "create:fluid_pipe",
    "create:encased_fluid_pipe",
    "create:glass_fluid_pipe",
    "create:smart_fluid_pipe",
    "create:fluid_valve",
];

#[derive(Default)]
struct Create;

impl Plugin for Create {
    fn init(&mut self) -> Registration {
        Registration {
            block_entities: HANDLED.iter().map(|s| (*s).to_owned()).collect(),
            replaces_block_model: REPLACES_BLOCK_MODEL
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            sprites: tank::SPRITES
                .iter()
                .map(|s| (*s).to_owned())
                .chain(belt::sprites())
                .collect(),
        }
    }

    fn block_entity(&mut self, view: &BlockEntityView) -> Rig {
        let parts = match view.kind.as_str() {
            "create:simple_kinetic" => kinetic::simple(view),
            "create:encased_shaft" | "create:adjustable_chain_gearshift" => {
                kinetic::encased_shaft(view)
            }
            "create:encased_cogwheel" => kinetic::encased_cog(view, false),
            "create:encased_large_cogwheel" => kinetic::encased_cog(view, true),
            "create:gearbox" => kinetic::gearbox(view),
            "create:clutch" | "create:gearshift" | "create:sequenced_gearshift" => {
                kinetic::split_shaft(view)
            }
            "create:motor" => kinetic::motor(view),
            "create:turntable"
            | "create:gantry_shaft"
            | "create:powered_shaft"
            | "create:crushing_wheel" => kinetic::plain(view),
            "create:water_wheel" => kinetic::water_wheel(view, false),
            "create:large_water_wheel" => kinetic::water_wheel(view, true),
            "create:fluid_tank" | "create:creative_fluid_tank" => tank::tank(view),
            "create:belt" => belt::belt(view),
            "create:mechanical_pump" => {
                let mut parts = kinetic::pump(view);
                parts.extend(pipe::attachments(view, |d| pipe::neighbour(view.pos, d)));
                parts
            }
            "create:fluid_pipe"
            | "create:encased_fluid_pipe"
            | "create:glass_fluid_pipe"
            | "create:smart_fluid_pipe"
            | "create:fluid_valve" => pipe::attachments(view, |d| pipe::neighbour(view.pos, d)),
            "create:hand_crank" => kinetic::hand_crank(view),
            "create:rotation_speed_controller" => kinetic::speed_controller(view),
            "create:mechanical_bearing" | "create:windmill_bearing" => kinetic::bearing(view),
            _ => Vec::new(),
        };
        Rig { parts }
    }
}

export_plugin!(Create);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_replaced_type_is_handled() {
        for kind in REPLACES_BLOCK_MODEL {
            assert!(HANDLED.contains(kind), "{kind}");
        }
    }
}
