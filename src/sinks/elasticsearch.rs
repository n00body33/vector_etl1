use crate::{
    buffers::Acker,
    record::Record,
    sinks::util::{
        http::{HttpRetryLogic, HttpService},
        retries::FixedRetryPolicy,
        BatchServiceSink, Buffer, Compression, SinkExt,
    },
};
use futures::{Future, Sink};
use http::{Method, Uri};
use hyper::{Body, Client, Request};
use hyper_tls::HttpsConnector;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use tower::ServiceBuilder;

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct ElasticSearchConfig {
    pub host: String,
    pub index: String,
    pub doc_type: String,
    pub id_key: Option<String>,
    pub buffer_size: Option<usize>,
    pub compression: Option<Compression>,

    // Tower Request based configuration
    pub request_in_flight_limit: Option<usize>,
    pub request_timeout_secs: Option<u64>,
    pub request_rate_limit_duration_secs: Option<u64>,
    pub request_rate_limit_num: Option<u64>,
    pub request_retry_attempts: Option<usize>,
    pub request_retry_backoff_secs: Option<u64>,
}

#[typetag::serde(name = "elasticsearch")]
impl crate::topology::config::SinkConfig for ElasticSearchConfig {
    fn build(&self, acker: Acker) -> Result<(super::RouterSink, super::Healthcheck), String> {
        let sink = es(self.clone(), acker);
        let healtcheck = healthcheck(self.host.clone());

        Ok((sink, healtcheck))
    }
}

fn es(config: ElasticSearchConfig, acker: Acker) -> super::RouterSink {
    let host = config.host.clone();
    let id_key = config.id_key.clone();
    let buffer_size = config.buffer_size.unwrap_or(2 * 1024 * 1024);
    let gzip = match config.compression.unwrap_or(Compression::Gzip) {
        Compression::None => false,
        Compression::Gzip => true,
    };

    let timeout = config.request_timeout_secs.unwrap_or(10);
    let in_flight_limit = config.request_in_flight_limit.unwrap_or(1);
    let rate_limit_duration = config.request_rate_limit_duration_secs.unwrap_or(1);
    let rate_limit_num = config.request_rate_limit_num.unwrap_or(10);
    let retry_attempts = config.request_retry_attempts.unwrap_or(5);
    let retry_backoff_secs = config.request_retry_backoff_secs.unwrap_or(1);

    let policy = FixedRetryPolicy::new(
        retry_attempts,
        Duration::from_secs(retry_backoff_secs),
        HttpRetryLogic,
    );

    let http_service = HttpService::new(move |body: Vec<u8>| {
        let uri = format!("{}/_bulk", host);
        let uri: Uri = uri.parse().unwrap();

        let mut builder = hyper::Request::builder();
        builder.method(Method::POST);
        builder.uri(uri);

        builder.header("Content-Type", "application/x-ndjson");

        if gzip {
            builder.header("Content-Encoding", "gzip");
        }

        builder.body(body.into()).unwrap()
    });

    let service = ServiceBuilder::new()
        .in_flight_limit(in_flight_limit)
        .rate_limit(rate_limit_num, Duration::from_secs(rate_limit_duration))
        .retry(policy)
        .timeout(Duration::from_secs(timeout))
        .service(http_service)
        .expect("This is a bug, there is no spawning");

    let sink = BatchServiceSink::new(service, acker)
        .batched(Buffer::new(gzip), buffer_size)
        .with(move |record: Record| {
            let mut action = json!({
                "index": {
                    "_index": config.index,
                    "_type": config.doc_type,
                }
            });
            maybe_set_id(
                id_key.as_ref(),
                action.pointer_mut("/index").unwrap(),
                &record,
            );

            let mut body = serde_json::to_vec(&action).unwrap();
            body.push(b'\n');

            serde_json::to_writer(&mut body, &record).unwrap();
            body.push(b'\n');
            Ok(body)
        });

    Box::new(sink)
}

fn healthcheck(host: String) -> super::Healthcheck {
    let uri = format!("{}/_cluster/health", host);
    let request = Request::get(uri).body(Body::empty()).unwrap();

    let https = HttpsConnector::new(4).expect("TLS initialization failed");
    let client = Client::builder().build(https);
    let healthcheck = client
        .request(request)
        .map_err(|err| err.to_string())
        .and_then(|response| {
            if response.status() == hyper::StatusCode::OK {
                Ok(())
            } else {
                Err(format!("Unexpected status: {}", response.status()))
            }
        });

    Box::new(healthcheck)
}

fn maybe_set_id(key: Option<impl AsRef<str>>, doc: &mut serde_json::Value, record: &Record) {
    if let Some(val) = key.and_then(|k| record.get(&k.as_ref().into())) {
        let val = val.to_string_lossy();

        doc.as_object_mut()
            .unwrap()
            .insert("_id".into(), json!(val));
    }
}

#[cfg(test)]
mod tests {
    use super::maybe_set_id;
    use crate::Record;
    use serde_json::json;

    #[test]
    fn sets_id_from_custom_field() {
        let id_key = Some("foo");
        let mut record = Record::from("butts");
        record.insert("foo".into(), "bar".into());
        let mut action = json!({});

        maybe_set_id(id_key, &mut action, &record);

        assert_eq!(json!({"_id": "bar"}), action);
    }

    #[test]
    fn doesnt_set_id_when_field_missing() {
        let id_key = Some("foo");
        let mut record = Record::from("butts");
        record.insert("not_foo".into(), "bar".into());
        let mut action = json!({});

        maybe_set_id(id_key, &mut action, &record);

        assert_eq!(json!({}), action);
    }

    #[test]
    fn doesnt_set_id_when_not_configured() {
        let id_key: Option<&str> = None;
        let mut record = Record::from("butts");
        record.insert("foo".into(), "bar".into());
        let mut action = json!({});

        maybe_set_id(id_key, &mut action, &record);

        assert_eq!(json!({}), action);
    }
}

#[cfg(test)]
#[cfg(feature = "es-integration-tests")]
mod integration_tests {
    use super::ElasticSearchConfig;
    use crate::buffers::Acker;
    use crate::{
        record,
        test_util::{block_on, random_records_with_stream, random_string},
        topology::config::SinkConfig,
        Record,
    };
    use elastic::client::SyncClientBuilder;
    use futures::{Future, Sink};
    use hyper::{Body, Client, Request};
    use hyper_tls::HttpsConnector;
    use serde_json::{json, Value};

    #[test]
    fn structures_records_correctly() {
        let index = gen_index();
        let config = ElasticSearchConfig {
            host: "http://localhost:9200/".into(),
            index: index.clone(),
            doc_type: "log_lines".into(),
            id_key: Some("my_id".into()),
            ..Default::default()
        };

        let (sink, _hc) = config.build(Acker::Null).unwrap();

        let mut input_record = Record::from("raw log line");
        input_record.insert("my_id".into(), "42".into());
        input_record.insert("foo".into(), "bar".into());

        let pump = sink.send(input_record.clone());
        block_on(pump).unwrap();

        // make sure writes all all visible
        block_on(flush(config.host)).unwrap();

        let client = SyncClientBuilder::new().build().unwrap();

        let response = client
            .search::<Value>()
            .index(index)
            .body(json!({
                "query": { "query_string": { "query": "*" } }
            }))
            .send()
            .unwrap();
        assert_eq!(1, response.total());

        let hit = response.into_hits().next().unwrap();
        assert_eq!("42", hit.id());

        let value = hit.into_document().unwrap();
        let expected = json!({
            "message": "raw log line",
            "my_id": "42",
            "foo": "bar",
            "timestamp": input_record[&record::TIMESTAMP],
        });
        assert_eq!(expected, value);
    }

    #[test]
    fn insert_records() {
        let index = gen_index();
        let config = ElasticSearchConfig {
            host: "http://localhost:9200/".into(),
            index: index.clone(),
            doc_type: "log_lines".into(),
            ..Default::default()
        };

        let (sink, _hc) = config.build(Acker::Null).unwrap();

        let (input, records) = random_records_with_stream(100, 100);

        let pump = sink.send_all(records);
        block_on(pump).unwrap();

        // make sure writes all all visible
        block_on(flush(config.host)).unwrap();

        let client = SyncClientBuilder::new().build().unwrap();

        let response = client
            .search::<Value>()
            .index(index)
            .body(json!({
                "query": { "query_string": { "query": "*" } }
            }))
            .send()
            .unwrap();

        assert_eq!(input.len() as u64, response.total());
        let input = input
            .into_iter()
            .map(|rec| serde_json::to_value(rec).unwrap())
            .collect::<Vec<_>>();
        for hit in response.into_hits() {
            let record = hit.into_document().unwrap();
            assert!(input.contains(&record));
        }
    }

    fn gen_index() -> String {
        format!("test-{}", random_string(10).to_lowercase())
    }

    fn flush(host: String) -> impl Future<Item = (), Error = String> {
        let uri = format!("{}/_flush", host);
        let request = Request::post(uri).body(Body::empty()).unwrap();

        let https = HttpsConnector::new(4).expect("TLS initialization failed");
        let client = Client::builder().build(https);
        client
            .request(request)
            .map_err(|err| err.to_string())
            .and_then(|response| {
                if response.status() == hyper::StatusCode::OK {
                    Ok(())
                } else {
                    Err(format!("Unexpected status: {}", response.status()))
                }
            })
    }

}
