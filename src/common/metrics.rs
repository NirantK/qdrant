use prometheus::proto::{Counter, Gauge, LabelPair, Metric, MetricFamily, MetricType};
use prometheus::TextEncoder;

use crate::common::telemetry::TelemetryData;
use crate::common::telemetry_ops::app_telemetry::AppBuildTelemetry;
use crate::common::telemetry_ops::cluster_telemetry::{ClusterStatusTelemetry, ClusterTelemetry};
use crate::common::telemetry_ops::collections_telemetry::{
    CollectionTelemetryEnum, CollectionsTelemetry,
};
use crate::common::telemetry_ops::requests_telemetry::{
    GrpcTelemetry, RequestsTelemetry, WebApiTelemetry,
};

/// Encapsulates metrics data in Prometheus format.
pub struct MetricsData {
    metrics: Vec<MetricFamily>,
}

impl MetricsData {
    pub fn format_metrics(&self) -> String {
        TextEncoder::new().encode_to_string(&self.metrics).unwrap()
    }
}

impl From<TelemetryData> for MetricsData {
    fn from(telemetry_data: TelemetryData) -> Self {
        let mut metrics = vec![];
        telemetry_data.add_metrics(&mut metrics);
        Self { metrics }
    }
}

trait MetricsProvider {
    /// Add metrics definitions for this.
    fn add_metrics(&self, metrics: &mut Vec<MetricFamily>);
}

impl MetricsProvider for TelemetryData {
    fn add_metrics(&self, metrics: &mut Vec<MetricFamily>) {
        self.app.add_metrics(metrics);
        self.collections.add_metrics(metrics);
        self.cluster.add_metrics(metrics);
        self.requests.add_metrics(metrics);
    }
}

impl MetricsProvider for AppBuildTelemetry {
    fn add_metrics(&self, metrics: &mut Vec<MetricFamily>) {
        metrics.push(metric_family(
            "app_info",
            "information about qdrant server",
            MetricType::COUNTER,
            vec![counter(
                1.0,
                &[("name", &self.name), ("version", &self.version)],
            )],
        ));
    }
}

impl MetricsProvider for CollectionsTelemetry {
    fn add_metrics(&self, metrics: &mut Vec<MetricFamily>) {
        metrics.push(metric_family(
            "collections_total",
            "number of collections",
            MetricType::GAUGE,
            vec![gauge(self.number_of_collections as f64, &[])],
        ));

        // Count collection types
        if let Some(ref collections) = self.collections {
            let full_count = collections
                .iter()
                .filter(|p| matches!(p, CollectionTelemetryEnum::Full(_)))
                .count();
            let aggregated_count = collections
                .iter()
                .filter(|p| matches!(p, CollectionTelemetryEnum::Aggregated(_)))
                .count();
            metrics.push(metric_family(
                "collections_full_total",
                "number of full collections",
                MetricType::GAUGE,
                vec![gauge(full_count as f64, &[])],
            ));
            metrics.push(metric_family(
                "collections_aggregated_total",
                "number of aggregated collections",
                MetricType::GAUGE,
                vec![gauge(aggregated_count as f64, &[])],
            ));
        }
    }
}

impl MetricsProvider for ClusterTelemetry {
    fn add_metrics(&self, metrics: &mut Vec<MetricFamily>) {
        metrics.push(metric_family(
            "cluster_enabled",
            "is cluster support enabled",
            MetricType::COUNTER,
            vec![counter(if self.enabled { 1.0 } else { 0.0 }, &[])],
        ));

        if let Some(ref status) = self.status {
            status.add_metrics(metrics);
        }
    }
}

impl MetricsProvider for ClusterStatusTelemetry {
    fn add_metrics(&self, metrics: &mut Vec<MetricFamily>) {
        metrics.push(metric_family(
            "cluster_peers_total",
            "total number of cluster peers",
            MetricType::GAUGE,
            vec![gauge(self.number_of_peers as f64, &[])],
        ));
        metrics.push(metric_family(
            "cluster_term",
            "current cluster term",
            MetricType::COUNTER,
            vec![counter(self.term as f64, &[])],
        ));

        if let Some(ref peer_id) = self.peer_id.map(|p| p.to_string()) {
            metrics.push(metric_family(
                "cluster_commit",
                "index of last committed (finalized) operation cluster peer is aware of",
                MetricType::COUNTER,
                vec![counter(self.commit as f64, &[("peer_id", peer_id)])],
            ));
            metrics.push(metric_family(
                "cluster_pending_operations_total",
                "total number of pending operations for cluster peer",
                MetricType::GAUGE,
                vec![gauge(self.pending_operations as f64, &[])],
            ));
            metrics.push(metric_family(
                "cluster_voter",
                "is cluster peer a voter or learner",
                MetricType::GAUGE,
                vec![gauge(if self.is_voter { 1.0 } else { 0.0 }, &[])],
            ));
        }
    }
}

impl MetricsProvider for RequestsTelemetry {
    fn add_metrics(&self, metrics: &mut Vec<MetricFamily>) {
        self.rest.add_metrics(metrics);
        self.grpc.add_metrics(metrics);
    }
}

impl MetricsProvider for WebApiTelemetry {
    fn add_metrics(&self, metrics: &mut Vec<MetricFamily>) {
        // Skip if there are no request stats
        if self.responses.is_empty() || self.responses.iter().all(|r| r.1.is_empty()) {
            return;
        }

        let (mut total, mut fail_total, mut avg_secs, mut min_secs, mut max_secs) =
            (vec![], vec![], vec![], vec![], vec![]);
        for (endpoint, responses) in &self.responses {
            let (method, endpoint) = endpoint.split_once(' ').unwrap();
            for (status, stats) in responses {
                let labels = [
                    ("method", method),
                    ("endpoint", endpoint),
                    ("status", &status.to_string()),
                ];
                total.push(counter(stats.count as f64, &labels));
                fail_total.push(counter(stats.fail_count as f64, &labels));
                avg_secs.push(gauge(
                    stats.avg_duration_micros.unwrap_or(0.0) as f64 / 1_000_000.0,
                    &labels,
                ));
                min_secs.push(gauge(
                    stats.min_duration_micros.unwrap_or(0.0) as f64 / 1_000_000.0,
                    &labels,
                ));
                max_secs.push(gauge(
                    stats.max_duration_micros.unwrap_or(0.0) as f64 / 1_000_000.0,
                    &labels,
                ));
            }
        }

        metrics.push(metric_family(
            "rest_responses_total",
            "total number of responses",
            MetricType::COUNTER,
            total,
        ));
        metrics.push(metric_family(
            "rest_responses_fail_total",
            "total number of failed responses",
            MetricType::COUNTER,
            fail_total,
        ));
        metrics.push(metric_family(
            "rest_responses_avg_duration_seconds",
            "average response duration",
            MetricType::GAUGE,
            avg_secs,
        ));
        metrics.push(metric_family(
            "rest_responses_min_duration_seconds",
            "minimum response duration",
            MetricType::GAUGE,
            min_secs,
        ));
        metrics.push(metric_family(
            "rest_responses_max_duration_seconds",
            "maximum response duration",
            MetricType::GAUGE,
            max_secs,
        ));
    }
}

impl MetricsProvider for GrpcTelemetry {
    fn add_metrics(&self, metrics: &mut Vec<MetricFamily>) {
        // Skip if there are no request stats
        if self.responses.is_empty() {
            return;
        }

        let (mut total, mut fail_total, mut avg_secs, mut min_secs, mut max_secs) =
            (vec![], vec![], vec![], vec![], vec![]);
        for (endpoint, stats) in &self.responses {
            let labels = [("endpoint", endpoint.as_str())];
            total.push(counter(stats.count as f64, &labels));
            fail_total.push(counter(stats.fail_count as f64, &labels));
            avg_secs.push(gauge(
                stats.avg_duration_micros.unwrap_or(0.0) as f64 / 1_000_000.0,
                &labels,
            ));
            min_secs.push(gauge(
                stats.min_duration_micros.unwrap_or(0.0) as f64 / 1_000_000.0,
                &labels,
            ));
            max_secs.push(gauge(
                stats.max_duration_micros.unwrap_or(0.0) as f64 / 1_000_000.0,
                &labels,
            ));
        }

        metrics.push(metric_family(
            "grpc_responses_total",
            "total number of responses",
            MetricType::COUNTER,
            total,
        ));
        metrics.push(metric_family(
            "grpc_responses_fail_total",
            "total number of failed responses",
            MetricType::COUNTER,
            fail_total,
        ));
        metrics.push(metric_family(
            "grpc_responses_avg_duration_seconds",
            "average response duration",
            MetricType::GAUGE,
            avg_secs,
        ));
        metrics.push(metric_family(
            "grpc_responses_min_duration_seconds",
            "minimum response duration",
            MetricType::GAUGE,
            min_secs,
        ));
        metrics.push(metric_family(
            "grpc_responses_max_duration_seconds",
            "maximum response duration",
            MetricType::GAUGE,
            max_secs,
        ));
    }
}

fn metric_family(name: &str, help: &str, r#type: MetricType, metrics: Vec<Metric>) -> MetricFamily {
    let mut metric_family = MetricFamily::default();
    metric_family.set_name(name.into());
    metric_family.set_help(help.into());
    metric_family.set_field_type(r#type);
    metric_family.set_metric(metrics);
    metric_family
}

fn counter(value: f64, labels: &[(&str, &str)]) -> Metric {
    let mut metric = Metric::default();
    metric.set_label(labels.iter().map(|(n, v)| label_pair(n, v)).collect());
    metric.set_counter({
        let mut counter = Counter::default();
        counter.set_value(value);
        counter
    });
    metric
}

fn gauge(value: f64, labels: &[(&str, &str)]) -> Metric {
    let mut metric = Metric::default();
    metric.set_label(labels.iter().map(|(n, v)| label_pair(n, v)).collect());
    metric.set_gauge({
        let mut gauge = Gauge::default();
        gauge.set_value(value);
        gauge
    });
    metric
}

fn label_pair(name: &str, value: &str) -> LabelPair {
    let mut label = LabelPair::default();
    label.set_name(name.into());
    label.set_value(value.into());
    label
}
