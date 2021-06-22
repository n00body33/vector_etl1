use crate::{
    internal_events::{AggregateEventRecorded, AggregateFlushed, AggregateUpdateFailed},
    transforms::{
        TaskTransform,
        Transform,
    },
    config::{DataType, GlobalOptions, TransformConfig, TransformDescription},
    event::{
        metric,
        Event,
        EventMetadata,
    },
};
use async_stream::stream;
use futures::{stream, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::Entry, HashMap},
    pin::Pin,
    time::{Duration},
};

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct AggregateConfig {
    /// The interval between flushes in milliseconds.
    #[serde(default = "default_interval_ms")]
    pub interval_ms: u64,
}

fn default_interval_ms() -> u64 {
    10 * 1000
}

inventory::submit! {
    TransformDescription::new::<AggregateConfig>("aggregate")
}

impl_generate_config_from_default!(AggregateConfig);

#[async_trait::async_trait]
#[typetag::serde(name = "aggregate")]
impl TransformConfig for AggregateConfig {
    async fn build(&self, _globals: &GlobalOptions) -> crate::Result<Transform> {
        Aggregate::new(self).map(Transform::task)
    }

    fn input_type(&self) -> DataType {
        DataType::Metric
    }

    fn output_type(&self) -> DataType {
        DataType::Metric
    }

    fn transform_type(&self) -> &'static str {
        "aggregate"
    }
}

type MetricEntry = (metric::MetricData, EventMetadata);

//------------------------------------------------------------------------------

#[derive(Debug)]
pub struct Aggregate {
    interval: Duration,
    map: HashMap<metric::MetricSeries, MetricEntry>,
}

impl Aggregate {
    pub fn new(config: &AggregateConfig) -> crate::Result<Self> {
        Ok(Self {
            interval: Duration::from_millis(config.interval_ms),
            map: HashMap::new(),
        })
    }

    fn record(&mut self, event: Event) {
        let (series, data, metadata) = event.into_metric().into_parts();

        match data.kind {
            metric::MetricKind::Incremental => {
                match self.map.entry(series) {
                    Entry::Occupied(mut entry) => {
                        let existing = entry.get_mut();
                        if ! existing.0.update(&data) {
                            emit!(AggregateUpdateFailed);
                        }
                        existing.1.merge(metadata);
                    }
                    Entry::Vacant(entry) => {
                        entry.insert((data, metadata));
                    },
                }
            },
            metric::MetricKind::Absolute => {
                // Always replace/store
                self.map.insert(series, (data, metadata));
            }
        };

        emit!(AggregateEventRecorded);
    }

    fn flush_into(&mut self, output: &mut Vec<Event>) {
        for (series, entry) in self.map.drain() {
            let metric = metric::Metric::from_parts(series, entry.0, entry.1);
            output.push(Event::Metric(metric));
        }

        emit!(AggregateFlushed);
    }
}

impl TaskTransform for Aggregate {
    fn transform(
        mut self: Box<Self>,
        mut input_rx: Pin<Box<dyn Stream<Item = Event> + Send>>,
    ) -> Pin<Box<dyn Stream<Item = Event> + Send>>
    where
        Self: 'static,
    {
        let mut flush_stream = tokio::time::interval(self.interval);

        Box::pin(
            stream! {
                let mut output = Vec::new();
                let mut done = false;
                while !done {
                    tokio::select! {
                        _ = flush_stream.tick() => {
                            self.flush_into(&mut output);
                        },
                        maybe_event = input_rx.next() => {
                            match maybe_event {
                                None => {
                                    self.flush_into(&mut output);
                                    done = true;
                                }
                                Some(event) => self.record(event),
                            }
                        }
                    };
                    for event in output.drain(..) {
                        yield event;
                    }
                }
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event::metric, event::Event, event::Metric};
    use futures::SinkExt;
    use std::task::Poll;

    #[test]
    fn generate_config() {
        crate::test_util::test_generate_config::<AggregateConfig>();
    }

    fn make_metric(
        name: &'static str,
        kind: metric::MetricKind,
        value: metric::MetricValue,
    ) -> Event {
        Event::Metric(
            Metric::new(
                name,
                kind,
                value,
            )
        )
    }

    #[test]
    fn incremental() {
        let mut agg = Aggregate::new(&AggregateConfig { interval_ms: 1000_u64 }).unwrap();

        let counter_a_1 = make_metric("counter_a", metric::MetricKind::Incremental,
            metric::MetricValue::Counter { value: 42.0 });
        let counter_a_2 = make_metric("counter_a", metric::MetricKind::Incremental,
            metric::MetricValue::Counter { value: 43.0 });
        let counter_a_summed = make_metric("counter_a", metric::MetricKind::Incremental,
            metric::MetricValue::Counter { value: 85.0 });

        // Single item, just stored regardless of kind
        agg.record(counter_a_1.clone());
        let mut out = vec![];
        // We should flush 1 item counter_a_1
        agg.flush_into(&mut out);
        assert_eq!(1, out.len());
        assert_eq!(&counter_a_1, &out[0]);

        // A subsequent flush doesn't send out anything
        out.clear();
        agg.flush_into(&mut out);
        assert_eq!(0, out.len());

        // One more just to make sure that we don't re-see from the other buffer
        out.clear();
        agg.flush_into(&mut out);
        assert_eq!(0, out.len());

        // Two increments with the same series, should sum into 1
        agg.record(counter_a_1.clone());
        agg.record(counter_a_2.clone());
        out.clear();
        agg.flush_into(&mut out);
        assert_eq!(1, out.len());
        assert_eq!(&counter_a_summed, &out[0]);

        let counter_b_1 = make_metric("counter_b", metric::MetricKind::Incremental,
            metric::MetricValue::Counter { value: 44.0 });
        // Two increments with the different series, should get each back as-is
        agg.record(counter_a_1.clone());
        agg.record(counter_b_1.clone());
        out.clear();
        agg.flush_into(&mut out);
        assert_eq!(2, out.len());
        // B/c we don't know the order they'll come back
        for event in out {
            match event.as_metric().series().name.name.as_str() {
                "counter_a" => assert_eq!(counter_a_1, event),
                "counter_b" => assert_eq!(counter_b_1, event),
                _ => panic!("Unexpected metric name in aggregate output"),
            }
        }
    }

    #[test]
    fn absolute() {
        let mut agg = Aggregate::new(&AggregateConfig { interval_ms: 1000_u64 }).unwrap();

        let gauge_a_1 = make_metric("gauge_a", metric::MetricKind::Absolute,
            metric::MetricValue::Gauge { value: 42.0 });
        let gauge_a_2 = make_metric("gauge_a", metric::MetricKind::Absolute,
            metric::MetricValue::Gauge { value: 43.0 });

        // Single item, just stored regardless of kind
        agg.record(gauge_a_1.clone());
        let mut out = vec![];
        // We should flush 1 item gauge_a_1
        agg.flush_into(&mut out);
        assert_eq!(1, out.len());
        assert_eq!(&gauge_a_1, &out[0]);

        // A subsequent flush doesn't send out anything
        out.clear();
        agg.flush_into(&mut out);
        assert_eq!(0, out.len());

        // One more just to make sure that we don't re-see from the other buffer
        out.clear();
        agg.flush_into(&mut out);
        assert_eq!(0, out.len());

        // Two absolutes with the same series, should get the 2nd (last) back.
        agg.record(gauge_a_1.clone());
        agg.record(gauge_a_2.clone());
        out.clear();
        agg.flush_into(&mut out);
        assert_eq!(1, out.len());
        assert_eq!(&gauge_a_2, &out[0]);

        let gauge_b_1 = make_metric("gauge_b", metric::MetricKind::Absolute,
            metric::MetricValue::Gauge { value: 44.0 });
        // Two increments with the different series, should get each back as-is
        agg.record(gauge_a_1.clone());
        agg.record(gauge_b_1.clone());
        out.clear();
        agg.flush_into(&mut out);
        assert_eq!(2, out.len());
        // B/c we don't know the order they'll come back
        for event in out {
            match event.as_metric().series().name.name.as_str() {
                "gauge_a" => assert_eq!(gauge_a_1, event),
                "gauge_b" => assert_eq!(gauge_b_1, event),
                _ => panic!("Unexpected metric name in aggregate output"),
            }
        }
    }

    #[tokio::test]
    async fn transform_shutdown() {
        let agg = toml::from_str::<AggregateConfig>(
            r#"
interval_ms = 999999
"#,
        )
        .unwrap()
        .build(&GlobalOptions::default())
        .await
        .unwrap();

        let agg = agg.into_task();

        let counter_a_1 = make_metric("counter_a", metric::MetricKind::Incremental,
            metric::MetricValue::Counter { value: 42.0 });
        let counter_a_2 = make_metric("counter_a", metric::MetricKind::Incremental,
            metric::MetricValue::Counter { value: 43.0 });
        let counter_a_summed = make_metric("counter_a", metric::MetricKind::Incremental,
            metric::MetricValue::Counter { value: 85.0 });
        let gauge_a_1 = make_metric("gauge_a", metric::MetricKind::Absolute,
            metric::MetricValue::Gauge { value: 42.0 });
        let gauge_a_2 = make_metric("gauge_a", metric::MetricKind::Absolute,
            metric::MetricValue::Gauge { value: 43.0 });
        let inputs = vec![counter_a_1, counter_a_2, gauge_a_1, gauge_a_2.clone()];

        // Queue up some events to be consummed & recorded
        let in_stream = Box::pin(stream::iter(inputs));
        // Kick off the transform process which should consume & record them
        let mut out_stream = agg.transform(in_stream);

        // B/c the input stream has ended we will have gone through the `input_rx.next() => None`
        // part of the loop and do the shutting down final flush immediately. We'll already be able
        // to read our expected bits on the output.
        let mut count = 0_u8;
        while let Some(event) = out_stream.next().await {
            count += 1;
            match event.as_metric().series().name.name.as_str() {
                "counter_a" => assert_eq!(counter_a_summed, event),
                "gauge_a" => assert_eq!(gauge_a_2, event),
                _ => panic!("Unexpected metric name in aggregate output"),
            };
        }
        // There were only 2
        assert_eq!(2, count);
    }

    #[tokio::test]
    async fn transform_interval() {
        let agg = toml::from_str::<AggregateConfig>(
            r#"
"#,
        )
        .unwrap()
        .build(&GlobalOptions::default())
        .await
        .unwrap();

        let agg = agg.into_task();

        let counter_a_1 = make_metric("counter_a", metric::MetricKind::Incremental,
            metric::MetricValue::Counter { value: 42.0 });
        let counter_a_2 = make_metric("counter_a", metric::MetricKind::Incremental,
            metric::MetricValue::Counter { value: 43.0 });
        let counter_a_summed = make_metric("counter_a", metric::MetricKind::Incremental,
            metric::MetricValue::Counter { value: 85.0 });
        let gauge_a_1 = make_metric("gauge_a", metric::MetricKind::Absolute,
            metric::MetricValue::Gauge { value: 42.0 });
        let gauge_a_2 = make_metric("gauge_a", metric::MetricKind::Absolute,
            metric::MetricValue::Gauge { value: 43.0 });

        let (mut tx, rx) = futures::channel::mpsc::channel(10);
        let mut out_stream = agg.transform(Box::pin(rx));

        tokio::time::pause();

        // tokio interval is always immediately ready, so we poll once to make sure
        // we trip it/set the interval in the future
        assert_eq!(Poll::Pending, futures::poll!(out_stream.next()));

        // Now send our events
        tx.send(counter_a_1.into()).await.unwrap();
        tx.send(counter_a_2.into()).await.unwrap();
        tx.send(gauge_a_1.into()).await.unwrap();
        tx.send(gauge_a_2.clone().into()).await.unwrap();
        // We won't have flushed yet b/c the interval hasn't elapsed, so no outputs
        assert_eq!(Poll::Pending, futures::poll!(out_stream.next()));
        // Now fast foward time enough that our flush should trigger.
        tokio::time::advance(Duration::from_secs(11)).await;
        // We should have had an interval fire now and our output aggregate events should be
        // available.
        let mut count = 0_u8;
        while count < 2 {
            if let Some(event) = out_stream.next().await {
                match event.as_metric().series().name.name.as_str() {
                    "counter_a" => assert_eq!(counter_a_summed, event),
                    "gauge_a" => assert_eq!(gauge_a_2, event),
                    _ => panic!("Unexpected metric name in aggregate output"),
                };
                count += 1;
            } else {
                panic!("Unexpectedly recieved None in output stream");
            }
        }
        // We should be back to pending, having nothing waiting for us
        assert_eq!(Poll::Pending, futures::poll!(out_stream.next()));
        // Close the input stream which should trigger the shutting down flush
        tx.disconnect();

        // And still nothing there
        assert_eq!(Poll::Ready(None), futures::poll!(out_stream.next()));
    }
}
