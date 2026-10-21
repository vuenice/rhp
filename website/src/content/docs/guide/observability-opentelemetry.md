---
title: "Observability & OpenTelemetry"
description: "Native distributed tracing, W3C context propagation, high-precision metrics, OTLP export, and APM error bridges."
section: "Language Guide"
order: 10
---

## Why Native OpenTelemetry in RHP?

Traditional PHP APM solutions (Datadog `ddtrace`, Sentry `ext-sentry`, New Relic `newrelic.so`) depend heavily on custom C extensions that hook internal Zend Engine function pointers. Because RHP runs on a high-performance register-based bytecode VM and fiber scheduler implemented in Rust, C extensions cannot run natively.

Instead of relying on fragile external C extensions, RHP embeds a **native, zero-cost OpenTelemetry engine** directly into the core runtime.

### Key Capabilities

1. **W3C Distributed Trace Context:** Automatic parsing, verification, and serialization of W3C `traceparent` (`00-{trace_id}-{span_id}-{flags}`) and `baggage` headers across services.
2. **Hierarchical Span Engine:** Nanosecond precision spans with parent-child nesting, status reporting, custom events, and exception recording.
3. **High-Precision Metrics & Histograms:** Counters, gauges, and memory-safe histograms computing count, sum, min, max, average, and exact percentiles ($p50$, $p90$, $p95$, $p99$).
4. **Canonical OTLP Exporters:** Real-time JSON export directly to `/v1/traces` and `/v1/metrics` endpoints (OpenTelemetry Collector, Jaeger, Honeycomb, Datadog, New Relic).
5. **Native APM Error Tracking Bridges:** Sentry event JSON and Datadog trace JSON generators that capture call stack frames and distributed trace identifiers during uncaught exceptions.
6. **Automatic VM Instrumentation:** Zero-configuration distributed tracing across worker requests, outbound `HTTP::get`/`HTTP::post` calls, and `DB::*` queries.

---

## The `Trace` & `Telemetry` Static Facades

In RHP script-land, distributed tracing and metrics are managed via Tier 1 static facades: `Trace` and `Telemetry` (an alias of `Trace`).

### 1. Closure-Based Span Helper (`Trace::span`)

The cleanest and safest way to trace a block of code is `Trace::span()`. It automatically starts a span, sets it as active in the current fiber context, captures any uncaught exceptions or panics as semantic error events, marks status as `OK` or `ERROR`, and ends the span cleanly:

```php
// Automatically instruments checkout operation
$response = Trace::span('checkout.process', function() {
    Trace::setAttribute('order.amount', 99.99);
    Trace::setAttribute('order.currency', 'USD');
    Trace::counter('orders.processed', 1);

    // Outbound HTTP call auto-instruments a client span and injects traceparent header
    $payment = HTTP::post('https://payment.internal/charge', [
        'amount' => 99.99,
    ]);

    Trace::addEvent('payment_received', Arr::dict([
        'gateway' => 'stripe',
        'status' => 'success',
    ]));

    return $payment->status();
}, Arr::dict(['category' => 'ecommerce']));
```

### 2. Manual Span Lifecycle (`Trace::startSpan` & `Trace::endSpan`)

For multi-step or asynchronous workflows spanning across functions:

```php
// Start a manual server span
Trace::startSpan('manual_user_sync', Arr::dict([
    'kind' => 'SERVER',
    'attributes' => Arr::dict([
        'sync.entity' => 'users',
        'sync.batch_size' => 100,
    ]),
]));

// Retrieve active distributed IDs for logging or RPC
$traceId = Trace::traceId();
$spanId  = Trace::spanId();
$parent  = Trace::parentSpanId();

Trace::addEvent('sync_step_completed', Arr::dict(['step' => 1]));

// Complete the span with status OK or ERROR
Trace::endSpan(Arr::dict(['status' => 'OK']));
```

### Supported Span Kinds

Span kinds conform strictly to the OpenTelemetry specification:
- `'INTERNAL'` (Default): Internal method or algorithmic block.
- `'SERVER'`: Handling incoming HTTP request, queue worker job, or RPC.
- `'CLIENT'`: Outgoing HTTP, database, or external cache call.
- `'PRODUCER'`: Message queue publication (e.g. Kafka, RabbitMQ).
- `'CONSUMER'`: Message queue ingestion.

---

## Distributed Context Propagation (W3C Standard)

RHP natively supports the **W3C Trace Context Level 1** specification:

### Header Formats

- **`traceparent`**: `00-{trace_id_32hex}-{span_id_16hex}-{flags_2hex}`  
  Example: `00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01`
- **`baggage`**: Key-value metadata pairs propagated across service boundaries.  
  Example: `userId=alice123,datacenter=us-east-1`

### Auto-Propagation in HTTP Client

When sending requests via `HTTP::get` or `HTTP::post`, RHP automatically injects the active span's `traceparent` and `baggage` into outbound HTTP request headers:

```php
Trace::span('call_downstream_service', function() {
    // RHP automatically injects header:
    // 'traceparent' => '00-{traceId}-{clientSpanId}-01'
    $res = HTTP::get('https://inventory.internal/stock');
    return $res->json();
});
```

---

## High-Precision Runtime Metrics & Histograms

The built-in metrics registry provides lightweight, thread-safe measurement primitives:

```php
// 1. Counter: monotonically increasing counters
Trace::counter('requests.total', 1.0);
Trace::counter('errors.count', 1.0);

// 2. Gauge: instantaneous point-in-time values
Trace::gauge('memory.allocated_mb', 128.5);
Trace::gauge('queue.active_jobs', 42);

// 3. Histogram: latency and distribution measurements
Trace::histogram('http.request_duration_ms', 12.4);
Trace::histogram('db.query_duration_ms', 3.8);

// 4. Retrieve exact percentile distribution summary
$summary = Trace::metricSummary('http.request_duration_ms');

Sys::print("Total Count: " . $summary['count']);
Sys::print("Average: " . $summary['avg'] . " ms");
Sys::print("p50: " . $summary['p50'] . " ms");
Sys::print("p95: " . $summary['p95'] . " ms");
Sys::print("p99: " . $summary['p99'] . " ms");
```

---

## Canonical OTLP Exporters

RHP natively exports spans and metrics directly into canonical OpenTelemetry Protocol (OTLP/HTTP JSON) format without third-party libraries:

```php
// Export /v1/traces payload
$otlpTracesJson = Trace::exportOtlp();

// Export /v1/metrics payload
$otlpMetricsJson = Trace::exportOtlpMetrics();
```

### Destination Compatibility

The generated JSON payloads match upstream OpenTelemetry specifications and are plug-and-play with:
- **OpenTelemetry Collector** (`POST /v1/traces`, `POST /v1/metrics`)
- **Jaeger** (OTLP ingestion port `4318`)
- **Honeycomb** (`https://api.honeycomb.io/v1/traces`)
- **Datadog Agent** (OTLP ingest endpoint)
- **New Relic** (`https://otlp.nr-data.net/v1/traces`)

---

## Native Error Tracking & APM Bridges (Sentry & Datadog)

### 1. Sentry Crash Reporting Bridge

Generate standard Sentry event payloads with stack traces and trace context on uncaught errors:

```php
try {
    $item = $cart->checkout();
} catch (Exception $e) {
    // Generates Sentry event JSON with exception type, value, and stack frames
    $sentryPayload = Trace::exportSentry(
        get_class($e),
        $e->getMessage()
    );
}
```

### 2. Datadog APM Tracing Bridge

Export completed spans in canonical Datadog APM trace format:

```php
$datadogTraces = Trace::exportDatadog();
```

---

## Automatic Worker Request Auto-Instrumentation

In production server mode (`rhp run build`):
1. **Root Server Span:** When a `WorkerRequest` arrives, RHP extracts any incoming W3C `traceparent` or `baggage` headers and starts a root `SERVER` span.
2. **Trace ID Forwarding:** The response (`WorkerResponse`) automatically includes `trace_id` and `span_id` headers.
3. **Automatic Latency Tracking:** Every completed request automatically updates the `rhp.request.duration_ms` histogram, `rhp.request.total` counter, and error counters without requiring a single line of user instrumentation.
