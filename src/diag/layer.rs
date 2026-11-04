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

/// Times the spans Bevy wraps its queue submit and present in, which no
/// `prof_span!` can reach, as timed spans in the performance report.
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
}

#[cfg(all(feature = "perf_report", not(target_arch = "wasm32")))]
impl<S> bevy::log::tracing_subscriber::Layer<S> for RenderSpans
where
    S: bevy::log::tracing::Subscriber
        + for<'a> bevy::log::tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_enter(
        &self,
        id: &bevy::log::tracing::span::Id,
        ctx: bevy::log::tracing_subscriber::layer::Context<'_, S>,
    ) {
        if !crate::diag::frame_profile::enabled() {
            return;
        }
        if let Some(span) = ctx.span(id)
            && Self::label(span.name()).is_some()
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
        let Some(label) = Self::label(span.name()) else {
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
