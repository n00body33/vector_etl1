#[cfg(test)]
mod tests {
    use vector_core::metric_tags;

    use super::*;
    use crate::{
        event::{metric, Event, Metric, MetricTags},
        test_util::components::assert_transform_compliance,
        transforms::{
            tag_cardinality_limit::{default_cache_size, BloomFilterConfig, Mode},
            test::create_topology,
        },
    };
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::ReceiverStream;

    #[test]
    fn generate_config() {
        crate::test_util::test_generate_config::<TagCardinalityLimitConfig>();
    }

    fn make_metric(tags: MetricTags) -> Event {
        Event::Metric(
            Metric::new(
                "event",
                metric::MetricKind::Incremental,
                metric::MetricValue::Counter { value: 1.0 },
            )
            .with_tags(Some(tags)),
        )
    }

    const fn make_transform_hashset(
        value_limit: u32,
        limit_exceeded_action: LimitExceededAction,
    ) -> TagCardinalityLimitConfig {
        TagCardinalityLimitConfig {
            value_limit,
            limit_exceeded_action,
            mode: Mode::Exact,
        }
    }

    const fn make_transform_bloom(
        value_limit: u32,
        limit_exceeded_action: LimitExceededAction,
    ) -> TagCardinalityLimitConfig {
        TagCardinalityLimitConfig {
            value_limit,
            limit_exceeded_action,
            mode: Mode::Probabilistic(BloomFilterConfig {
                cache_size_per_key: default_cache_size(),
            }),
        }
    }

    #[tokio::test]
    async fn tag_cardinality_limit_drop_event_hashset() {
        drop_event(make_transform_hashset(2, LimitExceededAction::DropEvent)).await;
    }

    #[tokio::test]
    async fn tag_cardinality_limit_drop_event_bloom() {
        drop_event(make_transform_bloom(2, LimitExceededAction::DropEvent)).await;
    }

    async fn drop_event(config: TagCardinalityLimitConfig) {
        assert_transform_compliance(async move {
            let event1 = make_metric(metric_tags!("tag1" => "val1"));
            let event2 = make_metric(metric_tags!("tag1" => "val2"));
            let event3 = make_metric(metric_tags!("tag1" => "val3"));

            let (tx, rx) = mpsc::channel(1);
            let (topology, mut out) = create_topology(ReceiverStream::new(rx), config).await;

            tx.send(event1.clone()).await.unwrap();
            tx.send(event2.clone()).await.unwrap();
            tx.send(event3.clone()).await.unwrap();

            let new_event1 = out.recv().await;
            let new_event2 = out.recv().await;

            drop(tx);
            topology.stop().await;

            let new_event3 = out.recv().await;

            assert_eq!(new_event1, Some(event1));
            assert_eq!(new_event2, Some(event2));
            // Third value rejected since value_limit is 2.
            assert_eq!(None, new_event3);
        })
        .await;
    }

    #[tokio::test]
    async fn tag_cardinality_limit_drop_tag_hashset() {
        drop_tag(make_transform_hashset(2, LimitExceededAction::DropTag)).await;
    }

    #[tokio::test]
    async fn tag_cardinality_limit_drop_tag_bloom() {
        drop_tag(make_transform_bloom(2, LimitExceededAction::DropTag)).await;
    }

    async fn drop_tag(config: TagCardinalityLimitConfig) {
        assert_transform_compliance(async move {
            let tags1 = metric_tags!("tag1" => "val1", "tag2" => "val1");
            let event1 = make_metric(tags1);

            let tags2 = metric_tags!("tag1" => "val2", "tag2" => "val1");
            let event2 = make_metric(tags2);

            let tags3 = metric_tags!("tag1" => "val3", "tag2" => "val1");
            let event3 = make_metric(tags3);

            let (tx, rx) = mpsc::channel(1);
            let (topology, mut out) = create_topology(ReceiverStream::new(rx), config).await;

            tx.send(event1.clone()).await.unwrap();
            tx.send(event2.clone()).await.unwrap();
            tx.send(event3.clone()).await.unwrap();

            let new_event1 = out.recv().await;
            let new_event2 = out.recv().await;
            let new_event3 = out.recv().await;

            drop(tx);
            topology.stop().await;

            assert_eq!(new_event1, Some(event1));
            assert_eq!(new_event2, Some(event2));
            // The third event should have been modified to remove "tag1"
            assert_ne!(new_event3, Some(event3));

            let new_event3 = new_event3.unwrap();
            assert!(!new_event3.as_metric().tags().unwrap().contains_key("tag1"));
            assert_eq!(
                "val1",
                new_event3.as_metric().tags().unwrap().get("tag2").unwrap()
            );
        })
        .await;
    }

    #[tokio::test]
    async fn tag_cardinality_limit_separate_value_limit_per_tag_hashset() {
        separate_value_limit_per_tag(make_transform_hashset(2, LimitExceededAction::DropEvent))
            .await;
    }

    #[tokio::test]
    async fn tag_cardinality_limit_separate_value_limit_per_tag_bloom() {
        separate_value_limit_per_tag(make_transform_bloom(2, LimitExceededAction::DropEvent)).await;
    }

    /// Test that hitting the value limit on one tag does not affect the ability to take new
    /// values for other tags.
    async fn separate_value_limit_per_tag(config: TagCardinalityLimitConfig) {
        assert_transform_compliance(async move {
            let event1 = make_metric(metric_tags!("tag1" => "val1", "tag2" => "val1"));

            let event2 = make_metric(metric_tags!("tag1" => "val2", "tag2" => "val1"));

            // Now value limit is reached for "tag1", but "tag2" still has values available.
            let event3 = make_metric(metric_tags!("tag1" => "val1", "tag2" => "val2"));

            let (tx, rx) = mpsc::channel(1);
            let (topology, mut out) = create_topology(ReceiverStream::new(rx), config).await;

            tx.send(event1.clone()).await.unwrap();
            tx.send(event2.clone()).await.unwrap();
            tx.send(event3.clone()).await.unwrap();

            let new_event1 = out.recv().await;
            let new_event2 = out.recv().await;
            let new_event3 = out.recv().await;

            drop(tx);
            topology.stop().await;

            assert_eq!(new_event1, Some(event1));
            assert_eq!(new_event2, Some(event2));
            assert_eq!(new_event3, Some(event3));
        })
        .await;
    }

    /// Test that hitting the value limit on one tag does not affect checking the limit on other
    /// tags that happen to be ordered later
    #[test]
    fn drop_event_checks_all_tags1() {
        drop_event_checks_all_tags(|val1, val2| metric_tags!("tag1" => val1, "tag2" => val2));
    }

    #[test]
    fn drop_event_checks_all_tags2() {
        drop_event_checks_all_tags(|val1, val2| metric_tags!("tag1" => val2, "tag2" => val1));
    }

    fn drop_event_checks_all_tags(make_tags: impl Fn(&str, &str) -> MetricTags) {
        let config = make_transform_hashset(2, LimitExceededAction::DropEvent);
        let mut transform = TagCardinalityLimit::new(config);

        let event1 = make_metric(make_tags("val1", "val1"));
        let event2 = make_metric(make_tags("val2", "val1"));
        // Next the limit is exceeded for the first tag.
        let event3 = make_metric(make_tags("val3", "val2"));
        // And then check if the new value for the second tag was not recorded by the above event.
        let event4 = make_metric(make_tags("val1", "val3"));

        let new_event1 = transform.transform_one(event1.clone());
        let new_event2 = transform.transform_one(event2.clone());
        let new_event3 = transform.transform_one(event3);
        let new_event4 = transform.transform_one(event4.clone());

        assert_eq!(new_event1, Some(event1));
        assert_eq!(new_event2, Some(event2));
        assert_eq!(new_event3, None);
        assert_eq!(new_event4, Some(event4));
    }
}
