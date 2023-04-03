//! Self registereing and deregistering metrics support.

use prometheus::{
    core::{Atomic, Collector, GenericCounter, GenericGauge},
    Counter, Gauge, Histogram, HistogramOpts, IntCounter, IntGauge, Registry,
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

    /// Returns a reference to the wrapped metric.
    #[inline]
    pub(crate) fn inner(&self) -> &T {
        self.metric.as_ref().expect("metric disappeared")
    }
}

impl<P> RegisteredMetric<GenericCounter<P>>
where
    P: Atomic,
{
    /// Increments the counter.
    #[inline]
    pub(crate) fn inc(&self) {
        self.inner().inc()
    }

    /// Increments the counter by set amount.
    #[inline]
    pub(crate) fn inc_by(&self, v: P::T) {
        self.inner().inc_by(v)
    }
}

impl<P> RegisteredMetric<GenericGauge<P>>
where
    P: Atomic,
{
    /// Decrements the gauge.
    #[inline]
    pub(crate) fn dec(&self) {
        self.inner().dec()
    }

    /// Returns the gauge value.
    #[cfg(test)]
    #[inline]
    pub(crate) fn get(&self) -> P::T {
        self.inner().get()
    }

    /// Increments the gauge.
    #[inline]
    pub(crate) fn inc(&self) {
        self.inner().inc()
    }

    /// Sets the gauge value.
    #[inline]
    pub(crate) fn set(&self, v: P::T) {
        self.inner().set(v)
    }
}

impl RegisteredMetric<Histogram> {
    /// Observes a given value.
    #[inline]
    pub(crate) fn observe(&self, v: f64) {
        self.inner().observe(v)
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
    fn new_counter<S1: Into<String>, S2: Into<String>>(
        &self,
        name: S1,
        help: S2,
    ) -> Result<RegisteredMetric<Counter>, prometheus::Error>;

    /// Creates a new [`Histogram`] registered to this registry.
    fn new_histogram<S1: Into<String>, S2: Into<String>>(
        &self,
        name: S1,
        help: S2,
        buckets: Vec<f64>,
    ) -> Result<RegisteredMetric<Histogram>, prometheus::Error>;

    /// Creates a new [`Gauge`] registered to this registry.
    fn new_gauge<S1: Into<String>, S2: Into<String>>(
        &self,
        name: S1,
        help: S2,
    ) -> Result<RegisteredMetric<Gauge>, prometheus::Error>;

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
    fn new_counter<S1: Into<String>, S2: Into<String>>(
        &self,
        name: S1,
        help: S2,
    ) -> Result<RegisteredMetric<Counter>, prometheus::Error> {
        RegisteredMetric::new(self.clone(), Counter::new(name, help)?)
    }

    fn new_gauge<S1: Into<String>, S2: Into<String>>(
        &self,
        name: S1,
        help: S2,
    ) -> Result<RegisteredMetric<Gauge>, prometheus::Error> {
        RegisteredMetric::new(self.clone(), Gauge::new(name, help)?)
    }

    fn new_histogram<S1: Into<String>, S2: Into<String>>(
        &self,
        name: S1,
        help: S2,
        buckets: Vec<f64>,
    ) -> Result<RegisteredMetric<Histogram>, prometheus::Error> {
        let histogram_opts = HistogramOpts::new(name, help).buckets(buckets);

        RegisteredMetric::new(self.clone(), Histogram::with_opts(histogram_opts)?)
    }

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
