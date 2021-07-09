use tracing_core::{span, subscriber, Event, Metadata, Subscriber};

pub (crate) struct NoSubscriber;

impl Subscriber for NoSubscriber {
    #[inline]
    fn register_callsite(&self, _: &'static Metadata<'static>) -> subscriber::Interest {
        subscriber::Interest::never()
    }

    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(0xDEAD)
    }

    fn event(&self, _event: &Event<'_>) {}

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    #[inline]
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        false
    }

    fn enter(&self, _span: &span::Id) {}
    fn exit(&self, _span: &span::Id) {}
}
