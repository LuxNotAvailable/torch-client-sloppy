//! The template port. A block entity of type `example:spinner` shows its
//! block's own model spinning about its `axis` property, at the speed its
//! NBT's `Speed` gives (revolutions per minute, as Create stores it).
//!
//! Copy this crate to start a port (`docs/porting/AGENT_GUIDE.md`).

use torch_mod_api::{Animation, BlockEntityView, Part, Plugin, Registration, Rig, export_plugin};

#[derive(Default)]
struct Example;

/// Degrees a part turns per tick at `rpm` revolutions per minute.
fn degrees_per_tick(rpm: f64) -> f32 {
    (rpm * 360.0 / 60.0 / 20.0) as f32
}

/// The unit vector of an `axis` property (`x`, `y`, `z`).
fn axis_vector(axis: Option<&str>) -> [f32; 3] {
    match axis {
        Some("x") => [1.0, 0.0, 0.0],
        Some("z") => [0.0, 0.0, 1.0],
        _ => [0.0, 1.0, 0.0],
    }
}

impl Plugin for Example {
    fn init(&mut self) -> Registration {
        Registration {
            block_entities: vec!["example:spinner".into()],
            ..Registration::default()
        }
    }

    fn block_entity(&mut self, view: &BlockEntityView) -> Rig {
        let speed = view.nbt_f64(&["Speed"]).unwrap_or(0.0);
        let (ns, path) = view
            .block
            .split_once(':')
            .unwrap_or(("minecraft", &view.block));
        let part = Part::model(format!("{ns}:block/{path}")).with_animation(Animation::Spin {
            axis: axis_vector(view.prop("axis")),
            pivot: [8.0; 3],
            degrees_per_tick: degrees_per_tick(speed),
            phase: 0.0,
        });
        Rig { parts: vec![part] }
    }
}

export_plugin!(Example);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_rpm_is_eighteen_degrees_a_tick() {
        assert_eq!(degrees_per_tick(60.0), 18.0);
    }

    #[test]
    fn a_spinner_spins_its_block_model_about_its_axis() {
        let view: BlockEntityView = serde_json::from_value(serde_json::json!({
            "type": "example:spinner", "pos": [0, 0, 0], "block": "example:wheel",
            "props": {"axis": "x"}, "nbt": {"Speed": 60.0}}))
        .unwrap();
        let rig = Example.block_entity(&view);
        assert_eq!(rig.parts.len(), 1);
        match &rig.parts[0].animation {
            Animation::Spin {
                axis,
                degrees_per_tick,
                ..
            } => {
                assert_eq!(*axis, [1.0, 0.0, 0.0]);
                assert_eq!(*degrees_per_tick, 18.0);
            }
            other => panic!("{other:?}"),
        }
    }
}
