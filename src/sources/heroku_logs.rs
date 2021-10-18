use crate::{
    config::{
        log_schema, DataType, GenerateConfig, Resource, SourceConfig, SourceContext,
        SourceDescription,
    },
    event::{Event, LogEvent},
    internal_events::{HerokuLogplexRequestReadError, HerokuLogplexRequestReceived},
    sources::util::{add_query_parameters, ErrorMessage, HttpSource, HttpSourceAuthConfig},
    tls::TlsConfig,
};
use bytes::{Buf, Bytes};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    net::SocketAddr,
    str::FromStr,
};

use warp::http::{HeaderMap, StatusCode};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct LogplexConfig {
    address: SocketAddr,
    #[serde(default)]
    query_parameters: Vec<String>,
    tls: Option<TlsConfig>,
    auth: Option<HttpSourceAuthConfig>,
}

inventory::submit! {
    SourceDescription::new::<LogplexConfig>("logplex")
}

inventory::submit! {
    SourceDescription::new::<LogplexConfig>("heroku_logs")
}

impl GenerateConfig for LogplexConfig {
    fn generate_config() -> toml::Value {
        toml::Value::try_from(Self {
            address: "0.0.0.0:80".parse().unwrap(),
            query_parameters: Vec::new(),
            tls: None,
            auth: None,
        })
        .unwrap()
    }
}

#[derive(Clone, Default)]
struct LogplexSource {
    query_parameters: Vec<String>,
}

impl HttpSource for LogplexSource {
    fn build_events(
        &self,
        body: Bytes,
        header_map: HeaderMap,
        query_parameters: HashMap<String, String>,
        _full_path: &str,
    ) -> Result<Vec<Event>, ErrorMessage> {
        let mut events = decode_message(body, header_map)?;
        add_query_parameters(&mut events, &self.query_parameters, query_parameters);
        Ok(events)
    }
}

#[async_trait::async_trait]
#[typetag::serde(name = "heroku_logs")]
impl SourceConfig for LogplexConfig {
    async fn build(&self, cx: SourceContext) -> crate::Result<super::Source> {
        let source = LogplexSource {
            query_parameters: self.query_parameters.clone(),
        };
        source.run(self.address, "events", true, &self.tls, &self.auth, cx)
    }

    fn output_type(&self) -> DataType {
        DataType::Log
    }

    fn source_type(&self) -> &'static str {
        "heroku_logs"
    }

    fn resources(&self) -> Vec<Resource> {
        vec![Resource::tcp(self.address)]
    }
}

// Add a compatibility alias to avoid breaking existing configs
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct LogplexCompatConfig(LogplexConfig);

#[async_trait::async_trait]
#[typetag::serde(name = "logplex")]
impl SourceConfig for LogplexCompatConfig {
    async fn build(&self, cx: SourceContext) -> crate::Result<super::Source> {
        self.0.build(cx).await
    }

    fn output_type(&self) -> DataType {
        self.0.output_type()
    }

    fn source_type(&self) -> &'static str {
        self.0.source_type()
    }

    fn resources(&self) -> Vec<Resource> {
        self.0.resources()
    }
}

fn decode_message(body: Bytes, header_map: HeaderMap) -> Result<Vec<Event>, ErrorMessage> {
    // Deal with headers
    let msg_count = match usize::from_str(get_header(&header_map, "Logplex-Msg-Count")?) {
        Ok(v) => v,
        Err(e) => return Err(header_error_message("Logplex-Msg-Count", &e.to_string())),
    };
    let frame_id = get_header(&header_map, "Logplex-Frame-Id")?;
    let drain_token = get_header(&header_map, "Logplex-Drain-Token")?;

    emit!(&HerokuLogplexRequestReceived {
        msg_count,
        frame_id,
        drain_token
    });

    // Deal with body
    let events = body_to_events(body);

    if events.len() != msg_count {
        let error_msg = format!(
            "Parsed event count does not match message count header: {} vs {}",
            events.len(),
            msg_count
        );

        if cfg!(test) {
            panic!("{}", error_msg);
        }
        return Err(header_error_message("Logplex-Msg-Count", &error_msg));
    }

    Ok(events)
}

fn get_header<'a>(header_map: &'a HeaderMap, name: &str) -> Result<&'a str, ErrorMessage> {
    if let Some(header_value) = header_map.get(name) {
        header_value
            .to_str()
            .map_err(|e| header_error_message(name, &e.to_string()))
    } else {
        Err(header_error_message(name, "Header does not exist"))
    }
}

fn header_error_message(name: &str, msg: &str) -> ErrorMessage {
    ErrorMessage::new(
        StatusCode::BAD_REQUEST,
        format!("Invalid request header {:?}: {:?}", name, msg),
    )
}

fn body_to_events(body: Bytes) -> Vec<Event> {
    let rdr = BufReader::new(body.reader());
    rdr.lines()
        .filter_map(|res| {
            res.map_err(|error| emit!(&HerokuLogplexRequestReadError { error }))
                .ok()
        })
        .filter(|s| !s.is_empty())
        .map(line_to_event)
        .collect()
}

fn line_to_event(line: String) -> Event {
    let parts = line.splitn(8, ' ').collect::<Vec<&str>>();

    let mut log = if parts.len() == 8 {
        let timestamp = parts[2];
        let hostname = parts[3];
        let app_name = parts[4];
        let proc_id = parts[5];
        let message = parts[7];

        let mut log = LogEvent::default();
        log.insert(log_schema().message_key(), message);

        if let Ok(ts) = timestamp.parse::<DateTime<Utc>>() {
            log.try_insert(log_schema().timestamp_key(), ts);
        }

        log.try_insert(log_schema().host_key(), hostname.to_owned());

        log.try_insert_flat("app_name", app_name.to_owned());
        log.try_insert_flat("proc_id", proc_id.to_owned());

        log
    } else {
        warn!(
            message = "Line didn't match expected logplex format, so raw message is forwarded.",
            fields = parts.len(),
            internal_log_rate_secs = 10
        );

        let mut log = LogEvent::default();
        log.insert(log_schema().message_key(), line);

        log
    };

    log.try_insert(log_schema().source_type_key(), Bytes::from("heroku_logs"));
    log.try_insert(log_schema().timestamp_key(), Utc::now());

    log.into()
}

#[cfg(test)]
mod tests {
    use super::{HttpSourceAuthConfig, LogplexConfig};
    use crate::{
        config::{log_schema, SourceConfig, SourceContext},
        test_util::{
            components, next_addr, random_string, spawn_collect_n, trace_init, wait_for_tcp,
        },
        Pipeline,
    };
    use chrono::{DateTime, Utc};
    use futures::Stream;
    use pretty_assertions::assert_eq;
    use std::net::SocketAddr;
    use vector_core::event::{Event, EventStatus, Value};

    #[test]
    fn generate_config() {
        crate::test_util::test_generate_config::<LogplexConfig>();
    }

    async fn source(
        auth: Option<HttpSourceAuthConfig>,
        query_parameters: Vec<String>,
        status: EventStatus,
        acknowledgements: bool,
    ) -> (impl Stream<Item = Event>, SocketAddr) {
        components::init();
        let (sender, recv) = Pipeline::new_test_finalize(status);
        let address = next_addr();
        let mut context = SourceContext::new_test(sender);
        context.acknowledgements = acknowledgements;
        tokio::spawn(async move {
            LogplexConfig {
                address,
                query_parameters,
                tls: None,
                auth,
            }
            .build(context)
            .await
            .unwrap()
            .await
            .unwrap()
        });
        wait_for_tcp(address).await;
        (recv, address)
    }

    async fn send(
        address: SocketAddr,
        body: &str,
        auth: Option<HttpSourceAuthConfig>,
        query: &str,
    ) -> u16 {
        let len = body.lines().count();
        let mut req = reqwest::Client::new().post(&format!("http://{}/events?{}", address, query));
        if let Some(auth) = auth {
            req = req.basic_auth(auth.username, Some(auth.password));
        }
        req.header("Logplex-Msg-Count", len)
            .header("Logplex-Frame-Id", "frame-foo")
            .header("Logplex-Drain-Token", "drain-bar")
            .body(body.to_owned())
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    fn make_auth() -> HttpSourceAuthConfig {
        HttpSourceAuthConfig {
            username: random_string(16),
            password: random_string(16),
        }
    }

    const SAMPLE_BODY: &str = r#"267 <158>1 2020-01-08T22:33:57.353034+00:00 host heroku router - at=info method=GET path="/cart_link" host=lumberjack-store.timber.io request_id=05726858-c44e-4f94-9a20-37df73be9006 fwd="73.75.38.87" dyno=web.1 connect=1ms service=22ms status=304 bytes=656 protocol=http"#;

    #[tokio::test]
    async fn logplex_handles_router_log() {
        trace_init();

        let auth = make_auth();

        let (rx, addr) = source(
            Some(auth.clone()),
            vec!["appname".to_string(), "absent".to_string()],
            EventStatus::Delivered,
            true,
        )
        .await;

        let mut events = spawn_collect_n(
            async move {
                assert_eq!(
                    200,
                    send(addr, SAMPLE_BODY, Some(auth), "appname=lumberjack-store").await
                )
            },
            rx,
            SAMPLE_BODY.lines().count(),
        )
        .await;
        components::SOURCE_TESTS.assert(&["http_path"]);

        let event = events.remove(0);
        let log = event.as_log();

        assert_eq!(
            log[log_schema().message_key()],
            r#"at=info method=GET path="/cart_link" host=lumberjack-store.timber.io request_id=05726858-c44e-4f94-9a20-37df73be9006 fwd="73.75.38.87" dyno=web.1 connect=1ms service=22ms status=304 bytes=656 protocol=http"#.into()
        );
        assert_eq!(
            log[log_schema().timestamp_key()],
            "2020-01-08T22:33:57.353034+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap()
                .into()
        );
        assert_eq!(log[&log_schema().host_key()], "host".into());
        assert_eq!(log[log_schema().source_type_key()], "heroku_logs".into());
        assert_eq!(log["appname"], "lumberjack-store".into());
        assert_eq!(log["absent"], Value::Null);
    }

    #[tokio::test]
    async fn logplex_handles_failures() {
        trace_init();

        let auth = make_auth();

        let (rx, addr) = source(Some(auth.clone()), vec![], EventStatus::Failed, true).await;

        let events = spawn_collect_n(
            async move {
                assert_eq!(
                    400,
                    send(addr, SAMPLE_BODY, Some(auth), "appname=lumberjack-store").await
                )
            },
            rx,
            SAMPLE_BODY.lines().count(),
        )
        .await;
        components::SOURCE_TESTS.assert(&["http_path"]);

        assert_eq!(events.len(), SAMPLE_BODY.lines().count());
    }

    #[tokio::test]
    async fn logplex_ignores_disabled_acknowledgements() {
        trace_init();

        let auth = make_auth();

        let (rx, addr) = source(Some(auth.clone()), vec![], EventStatus::Failed, false).await;

        let events = spawn_collect_n(
            async move {
                assert_eq!(
                    200,
                    send(addr, SAMPLE_BODY, Some(auth), "appname=lumberjack-store").await
                )
            },
            rx,
            SAMPLE_BODY.lines().count(),
        )
        .await;

        assert_eq!(events.len(), SAMPLE_BODY.lines().count());
    }

    #[tokio::test]
    async fn logplex_auth_failure() {
        let (_rx, addr) = source(Some(make_auth()), vec![], EventStatus::Delivered, true).await;

        assert_eq!(
            401,
            send(
                addr,
                SAMPLE_BODY,
                Some(make_auth()),
                "appname=lumberjack-store"
            )
            .await
        );
    }

    #[test]
    fn logplex_handles_normal_lines() {
        let body = "267 <158>1 2020-01-08T22:33:57.353034+00:00 host heroku router - foo bar baz";
        let event = super::line_to_event(body.into());
        let log = event.as_log();

        assert_eq!(log[log_schema().message_key()], "foo bar baz".into());
        assert_eq!(
            log[log_schema().timestamp_key()],
            "2020-01-08T22:33:57.353034+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap()
                .into()
        );
        assert_eq!(log[log_schema().host_key()], "host".into());
        assert_eq!(log[log_schema().source_type_key()], "heroku_logs".into());
    }

    #[test]
    fn logplex_handles_malformed_lines() {
        let body = "what am i doing here";
        let event = super::line_to_event(body.into());
        let log = event.as_log();

        assert_eq!(
            log[log_schema().message_key()],
            "what am i doing here".into()
        );
        assert!(log.get(log_schema().timestamp_key()).is_some());
        assert_eq!(log[log_schema().source_type_key()], "heroku_logs".into());
    }

    #[test]
    fn logplex_doesnt_blow_up_on_bad_framing() {
        let body = "1000000 <158>1 2020-01-08T22:33:57.353034+00:00 host heroku router - i'm not that long";
        let event = super::line_to_event(body.into());
        let log = event.as_log();

        assert_eq!(log[log_schema().message_key()], "i'm not that long".into());
        assert_eq!(
            log[log_schema().timestamp_key()],
            "2020-01-08T22:33:57.353034+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap()
                .into()
        );
        assert_eq!(log[log_schema().host_key()], "host".into());
        assert_eq!(log[log_schema().source_type_key()], "heroku_logs".into());
    }
}
