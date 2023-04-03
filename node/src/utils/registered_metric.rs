//! Self registereing and deregistering metrics support.

use prometheus::{
    core::{Atomic, Collector, GenericCounter},
    IntCounter, IntGauge, Registry,
};

/// A metric wrapper that will deregister the metric from a given registry on drop.
#[derive(Debug)]
pub(crate) struct RegisteredMetric<T>
where
    T: Collector + 'static,
{
    metric: Option<Box<T>>,
    registry: Registry,
}

impl<T> RegisteredMetric<T>
where
    T: Collector + 'static,
{
    /// Creates a new self-deregistering metric.
    pub(crate) fn new(registry: Registry, metric: T) -> Result<Self, prometheus::Error>
    where
        T: Clone,
    {
        let boxed_metric = Box::new(metric);
        registry.register(boxed_metric.clone())?;

        Ok(RegisteredMetric {
            metric: Some(boxed_metric),
            registry,
        })
    }

    /// Returns a reference to the inner metric.
    #[inline]
    fn inner(&self) -> &T {
        self.metric.as_ref().expect("metric disappeared")
    }
}

impl<P> RegisteredMetric<GenericCounter<P>>
where
    P: Atomic,
{
    /// Increment the counter.
    #[inline]
    pub(crate) fn inc(&self) {
        self.inner().inc()
    }
}

impl<T> Drop for RegisteredMetric<T>
where
    T: Collector + 'static,
{
    fn drop(&mut self) {
        if let Some(boxed_metric) = self.metric.take() {
            let desc = boxed_metric
                .desc()
                .iter()
                .next()
                .map(|desc| desc.fq_name.clone())
                .unwrap_or_default();
            self.registry.unregister(boxed_metric).unwrap_or_else(|_| {
                tracing::error!("unregistering {} failed: was not registered", desc)
            })
        }
    }
}

/// Extension trait for [`Registry`] instances.
pub(crate) trait RegistryExt {
    /// Creates a new [`IntCounter`] registered to this registry.
    fn new_int_counter<S1: Into<String>, S2: Into<String>>(
        &self,
        name: S1,
        help: S2,
    ) -> Result<RegisteredMetric<IntCounter>, prometheus::Error>;

    /// Creates a new [`IntGauge`] registered to this registry.
    fn new_int_gauge<S1: Into<String>, S2: Into<String>>(
        &self,
        name: S1,
        help: S2,
    ) -> Result<RegisteredMetric<IntGauge>, prometheus::Error>;
}

impl RegistryExt for Registry {
    fn new_int_counter<S1: Into<String>, S2: Into<String>>(
        &self,
        name: S1,
        help: S2,
    ) -> Result<RegisteredMetric<IntCounter>, prometheus::Error> {
        RegisteredMetric::new(self.clone(), IntCounter::new(name, help)?)
    }

    fn new_int_gauge<S1: Into<String>, S2: Into<String>>(
        &self,
        name: S1,
        help: S2,
    ) -> Result<RegisteredMetric<IntGauge>, prometheus::Error> {
        RegisteredMetric::new(self.clone(), IntGauge::new(name, help)?)
    }
}
