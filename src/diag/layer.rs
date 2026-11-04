use super::stats::{Stat, bump};

pub fn custom_layers(_app: &mut bevy::app::App) -> Option<bevy::log::BoxedLayer> {
    #[cfg_attr(
        all(
            target_arch = "wasm32",
            not(feature = "profiling"),
            not(feature = "perf_report")
        ),
        expect(unused_mut)
    )]
    let mut layers: Vec<bevy::log::BoxedLayer> = vec![Box::new(DropCounter)];
    #[cfg(not(target_arch = "wasm32"))]
    layers.push(Box::new(crate::diag::logfile::FileLayer));
    #[cfg(feature = "profiling")]
    layers.extend(crate::diag::profiling::bridge_layer());
    #[cfg(all(feature = "perf_report", not(target_arch = "wasm32")))]
    layers.push(Box::new(RenderSpans));
    Some(Box::new(layers))
}

struct DropCounter;

impl<S> bevy::log::tracing_subscriber::Layer<S> for DropCounter
where
    S: bevy::log::tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &bevy::log::tracing::Event<'_>,
        _ctx: bevy::log::tracing_subscriber::layer::Context<'_, S>,
    ) {
        if event.metadata().target() == "azalea_world::chunk::partial"
            && *event.metadata().level() == bevy::log::tracing::Level::WARN
        {
            let mut first_line = false;
            event.record(&mut FieldPeek {
                hit: &mut first_line,
            });
            if first_line {
                bump(Stat::ChunksDropped);
            }
        }
    }
}

struct FieldPeek<'a> {
    hit: &'a mut bool,
}

impl bevy::log::tracing::field::Visit for FieldPeek<'_> {
    fn record_debug(
        &mut self,
        field: &bevy::log::tracing::field::Field,
        value: &dyn std::fmt::Debug,
    ) {
        if field.name() == "message" && format!("{value:?}").contains("not in the render distance")
        {
            *self.hit = true;
        }
    }
}

/// Times the spans Bevy wraps its render graph nodes, queue submit and
/// present in, which no `prof_span!` can reach, as timed spans in the
/// performance report. The node spans come from `bevy_render/trace`.
#[cfg(all(feature = "perf_report", not(target_arch = "wasm32")))]
struct RenderSpans;

#[cfg(all(feature = "perf_report", not(target_arch = "wasm32")))]
impl RenderSpans {
    fn label(name: &str) -> Option<&'static str> {
        match name {
            "submit_graph_commands" => Some("render:queue_submit"),
            "present_frames" => Some("render:present"),
            _ => None,
        }
    }

    /// `node:<type>` for a render graph node's type name, interned: the
    /// profiler keys rows by `&'static str`, and there are a few dozen nodes.
    fn node_label(type_name: &str) -> &'static str {
        use std::collections::HashMap;
        use std::sync::{LazyLock, Mutex};
        static LABELS: LazyLock<Mutex<HashMap<String, &'static str>>> =
            LazyLock::new(Mutex::default);
        let Ok(mut labels) = LABELS.lock() else {
            return "node:?";
        };
        if let Some(label) = labels.get(type_name) {
            return label;
        }
        let label: &'static str = Box::leak(format!("node:{}", short_type(type_name)).into());
        labels.insert(type_name.to_owned(), label);
        label
    }
}

/// A node's type without module paths or the `ViewNodeRunner` wrapper:
/// `bevy_render::..::ViewNodeRunner<a::b::MainOpaquePass3dNode>` becomes
/// `MainOpaquePass3dNode`.
#[cfg(all(feature = "perf_report", not(target_arch = "wasm32")))]
fn short_type(type_name: &str) -> &str {
    let inner = match (type_name.find('<'), type_name.rfind('>')) {
        (Some(open), Some(close)) if type_name[..open].ends_with("NodeRunner") => {
            &type_name[open + 1..close]
        }
        _ => type_name,
    };
    let base = inner.split('<').next().unwrap_or(inner);
    base.rsplit("::").next().unwrap_or(base)
}

/// Reads a node span's `name` field.
#[cfg(all(feature = "perf_report", not(target_arch = "wasm32")))]
struct NodeName(Option<&'static str>);

#[cfg(all(feature = "perf_report", not(target_arch = "wasm32")))]
impl bevy::log::tracing::field::Visit for NodeName {
    fn record_str(&mut self, field: &bevy::log::tracing::field::Field, value: &str) {
        if field.name() == "name" {
            self.0 = Some(RenderSpans::node_label(value));
        }
    }

    fn record_debug(
        &mut self,
        field: &bevy::log::tracing::field::Field,
        value: &dyn std::fmt::Debug,
    ) {
        if field.name() == "name" && self.0.is_none() {
            self.0 = Some(RenderSpans::node_label(&format!("{value:?}")));
        }
    }
}

/// The label a span is timed under, kept on the span.
#[cfg(all(feature = "perf_report", not(target_arch = "wasm32")))]
struct Label(&'static str);

#[cfg(all(feature = "perf_report", not(target_arch = "wasm32")))]
impl<S> bevy::log::tracing_subscriber::Layer<S> for RenderSpans
where
    S: bevy::log::tracing::Subscriber
        + for<'a> bevy::log::tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &bevy::log::tracing::span::Attributes<'_>,
        id: &bevy::log::tracing::span::Id,
        ctx: bevy::log::tracing_subscriber::layer::Context<'_, S>,
    ) {
        let label = match attrs.metadata().name() {
            "node" => {
                let mut name = NodeName(None);
                attrs.record(&mut name);
                name.0
            }
            other => Self::label(other),
        };
        if let (Some(label), Some(span)) = (label, ctx.span(id)) {
            span.extensions_mut().replace(Label(label));
        }
    }

    fn on_enter(
        &self,
        id: &bevy::log::tracing::span::Id,
        ctx: bevy::log::tracing_subscriber::layer::Context<'_, S>,
    ) {
        if !crate::diag::frame_profile::enabled() {
            return;
        }
        if let Some(span) = ctx.span(id)
            && span.extensions().get::<Label>().is_some()
        {
            span.extensions_mut()
                .replace(crate::platform::time::Instant::now());
        }
    }

    fn on_exit(
        &self,
        id: &bevy::log::tracing::span::Id,
        ctx: bevy::log::tracing_subscriber::layer::Context<'_, S>,
    ) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let Some(label) = span.extensions().get::<Label>().map(|l| l.0) else {
            return;
        };
        if let Some(entered) = span
            .extensions_mut()
            .remove::<crate::platform::time::Instant>()
        {
            let ns = entered.elapsed().as_nanos() as u64;
            crate::diag::frame_profile::record(crate::diag::frame_profile::Group::Span, label, ns);
        }
    }
}

#[cfg(all(test, feature = "perf_report", not(target_arch = "wasm32")))]
mod tests {
    use super::short_type;

    #[test]
    fn a_node_is_named_by_its_own_type() {
        assert_eq!(
            short_type(
                "bevy_render::render_graph::node::ViewNodeRunner<bevy_core_pipeline::core_3d::main_opaque_pass_3d_node::MainOpaquePass3dNode>"
            ),
            "MainOpaquePass3dNode"
        );
        assert_eq!(
            short_type("torch_client::renderer::terrain_pool::cull::TerrainCullNode"),
            "TerrainCullNode"
        );
        assert_eq!(short_type("CameraDriverNode"), "CameraDriverNode");
    }
}
