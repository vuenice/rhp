//! High-Precision OpenTelemetry Metrics Engine
//!
//! Implements OpenTelemetry Metrics API & SDK v1.x:
//! - Monotonic Counters, UpDownCounters, and Gauges
//! - High-precision Histograms with explicit buckets and percentiles ($p50$, $p90$, $p95$, $p99$)
//! - Built-in runtime metric instruments:
//!   - Request latency percentiles (`rhp.request.duration_ms`)
//!   - Fiber queue depths (`rhp.fiber.queue_depth`)
//!   - Active worker connections (`rhp.worker.active_connections`)
//!   - JIT compile times (`rhp.jit.compile_time_us`)
//!   - Request memory allocations and arena reclaims (`rhp.arena.bytes_reclaimed`)

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use indexmap::IndexMap;

use crate::vm::value::PhpValue;

/// Default latency histogram bucket boundaries in milliseconds.
pub static DEFAULT_HISTOGRAM_BOUNDS: &[f64] = &[
    0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0,
];

/// An individual metric snapshot containing calculated percentiles and aggregations.
#[derive(Debug, Clone, PartialEq)]
pub struct HistogramSummary {
    pub name: String,
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub bucket_counts: Vec<u64>,
    pub explicit_bounds: Vec<f64>,
}

impl HistogramSummary {
    pub fn to_php_value(&self) -> PhpValue {
        let mut map = IndexMap::new();
        map.insert("name".to_string(), PhpValue::string(self.name.clone()));
        map.insert("count".to_string(), PhpValue::Int(self.count as i32));
        map.insert("sum".to_string(), PhpValue::Float(self.sum as f32));
        map.insert("min".to_string(), PhpValue::Float(self.min as f32));
        map.insert("max".to_string(), PhpValue::Float(self.max as f32));
        map.insert("avg".to_string(), PhpValue::Float(self.avg as f32));
        map.insert("p50".to_string(), PhpValue::Float(self.p50 as f32));
        map.insert("p90".to_string(), PhpValue::Float(self.p90 as f32));
        map.insert("p95".to_string(), PhpValue::Float(self.p95 as f32));
        map.insert("p99".to_string(), PhpValue::Float(self.p99 as f32));

        let bounds: Vec<PhpValue> = self
            .explicit_bounds
            .iter()
            .map(|&b| PhpValue::Float(b as f32))
            .collect();
        map.insert("explicit_bounds".to_string(), PhpValue::list(bounds));

        let buckets: Vec<PhpValue> = self
            .bucket_counts
            .iter()
            .map(|&c| PhpValue::Int(c as i32))
            .collect();
        map.insert("bucket_counts".to_string(), PhpValue::list(buckets));

        PhpValue::dict_from_map(map)
    }
}

/// In-memory Histogram accumulator.
#[derive(Debug, Clone)]
pub struct Histogram {
    pub name: String,
    pub samples: Vec<f64>,
    pub bounds: Vec<f64>,
    pub bucket_counts: Vec<u64>,
}

impl Histogram {
    pub fn new(name: impl Into<String>, bounds: Option<Vec<f64>>) -> Self {
        let b = bounds.unwrap_or_else(|| DEFAULT_HISTOGRAM_BOUNDS.to_vec());
        let count = b.len() + 1;
        Self {
            name: name.into(),
            samples: Vec::new(),
            bounds: b,
            bucket_counts: vec![0; count],
        }
    }

    pub fn record(&mut self, value: f64) {
        self.samples.push(value);
        // Find bucket index
        let mut idx = self.bounds.len();
        for (i, &bound) in self.bounds.iter().enumerate() {
            if value <= bound {
                idx = i;
                break;
            }
        }
        self.bucket_counts[idx] += 1;
    }

    pub fn summary(&self) -> HistogramSummary {
        let count = self.samples.len() as u64;
        if count == 0 {
            return HistogramSummary {
                name: self.name.clone(),
                count: 0,
                sum: 0.0,
                min: 0.0,
                max: 0.0,
                avg: 0.0,
                p50: 0.0,
                p90: 0.0,
                p95: 0.0,
                p99: 0.0,
                bucket_counts: self.bucket_counts.clone(),
                explicit_bounds: self.bounds.clone(),
            };
        }

        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let sum: f64 = sorted.iter().sum();
        let min = *sorted.first().unwrap_or(&0.0);
        let max = *sorted.last().unwrap_or(&0.0);
        let avg = sum / (count as f64);

        let percentile = |p: f64| -> f64 {
            if sorted.is_empty() {
                return 0.0;
            }
            let rank = p * (sorted.len() - 1) as f64;
            let low = rank.floor() as usize;
            let high = rank.ceil() as usize;
            if low == high {
                sorted[low]
            } else {
                let weight = rank - (low as f64);
                sorted[low] * (1.0 - weight) + sorted[high] * weight
            }
        };

        HistogramSummary {
            name: self.name.clone(),
            count,
            sum,
            min,
            max,
            avg,
            p50: percentile(0.50),
            p90: percentile(0.90),
            p95: percentile(0.95),
            p99: percentile(0.99),
            bucket_counts: self.bucket_counts.clone(),
            explicit_bounds: self.bounds.clone(),
        }
    }
}

/// Central Metrics Registry.
#[derive(Debug, Default)]
pub struct MetricsRegistry {
    counters: HashMap<String, f64>,
    gauges: HashMap<String, f64>,
    histograms: HashMap<String, Histogram>,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_counter(&mut self, name: &str, delta: f64) {
        let entry = self.counters.entry(name.to_string()).or_insert(0.0);
        *entry += delta;
    }

    pub fn record_gauge(&mut self, name: &str, value: f64) {
        self.gauges.insert(name.to_string(), value);
    }

    pub fn record_histogram(&mut self, name: &str, value: f64) {
        let entry = self
            .histograms
            .entry(name.to_string())
            .or_insert_with(|| Histogram::new(name, None));
        entry.record(value);
    }

    pub fn get_counter(&self, name: &str) -> Option<f64> {
        self.counters.get(name).copied()
    }

    pub fn get_gauge(&self, name: &str) -> Option<f64> {
        self.gauges.get(name).copied()
    }

    pub fn get_histogram_summary(&self, name: &str) -> Option<HistogramSummary> {
        self.histograms.get(name).map(|h| h.summary())
    }

    pub fn clear(&mut self) {
        self.counters.clear();
        self.gauges.clear();
        self.histograms.clear();
    }

    /// Snapshot all metrics into a script-land dictionary (`Arr::dict`).
    pub fn to_php_value(&self) -> PhpValue {
        let mut root = IndexMap::new();

        let mut counters_map = IndexMap::new();
        for (k, v) in &self.counters {
            counters_map.insert(k.clone(), PhpValue::Float(*v as f32));
        }
        root.insert(
            "counters".to_string(),
            PhpValue::dict_from_map(counters_map),
        );

        let mut gauges_map = IndexMap::new();
        for (k, v) in &self.gauges {
            gauges_map.insert(k.clone(), PhpValue::Float(*v as f32));
        }
        root.insert("gauges".to_string(), PhpValue::dict_from_map(gauges_map));

        let mut histograms_map = IndexMap::new();
        for (k, h) in &self.histograms {
            histograms_map.insert(k.clone(), h.summary().to_php_value());
        }
        root.insert(
            "histograms".to_string(),
            PhpValue::dict_from_map(histograms_map),
        );

        PhpValue::dict_from_map(root)
    }
}

static GLOBAL_METRICS: OnceLock<Mutex<MetricsRegistry>> = OnceLock::new();

pub fn get_metrics_registry() -> &'static Mutex<MetricsRegistry> {
    GLOBAL_METRICS.get_or_init(|| Mutex::new(MetricsRegistry::new()))
}

/// Increment a counter.
pub fn counter(name: &str, delta: f64) {
    if let Ok(mut reg) = get_metrics_registry().lock() {
        reg.record_counter(name, delta);
    }
}

/// Set a gauge value.
pub fn gauge(name: &str, value: f64) {
    if let Ok(mut reg) = get_metrics_registry().lock() {
        reg.record_gauge(name, value);
    }
}

/// Record a sample into a histogram.
pub fn histogram(name: &str, value: f64) {
    if let Ok(mut reg) = get_metrics_registry().lock() {
        reg.record_histogram(name, value);
    }
}

/// Retrieve all metrics as a script-land dictionary.
pub fn get_all_metrics() -> PhpValue {
    get_metrics_registry()
        .lock()
        .map(|r| r.to_php_value())
        .unwrap_or(PhpValue::Null)
}

/// Retrieve a histogram summary by name.
pub fn get_histogram_summary(name: &str) -> Option<HistogramSummary> {
    get_metrics_registry()
        .lock()
        .ok()
        .and_then(|r| r.get_histogram_summary(name))
}

/// Reset all metrics.
pub fn clear_metrics() {
    if let Ok(mut reg) = get_metrics_registry().lock() {
        reg.clear();
    }
}
