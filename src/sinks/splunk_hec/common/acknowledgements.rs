use std::{collections::HashMap, num::NonZeroU8, sync::Arc, time::Duration};

use hyper::Body;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc::UnboundedReceiver, oneshot::Sender};
use vector_core::event::EventStatus;

use crate::http::HttpClient;

use super::service::HttpRequestBuilder;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HecClientAcknowledgementsConfig {
    pub query_interval: NonZeroU8,
    pub retry_limit: NonZeroU8,
}

impl Default for HecClientAcknowledgementsConfig {
    fn default() -> Self {
        Self {
            query_interval: NonZeroU8::new(10).unwrap(),
            retry_limit: NonZeroU8::new(30).unwrap(),
        }
    }
}

pub fn default_hec_client_acknowledgements_config() -> Option<HecClientAcknowledgementsConfig> {
    Some(Default::default())
}

#[derive(Deserialize, Serialize, Eq, PartialEq, Debug)]
pub struct HecAckStatusRequest {
    pub acks: Vec<u64>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct HecAckStatusResponse {
    pub acks: HashMap<u64, bool>,
}

enum HecAckApiError {
    ClientBuildRequest,
    ClientParseResponse,
    ClientSendQuery,
    ServerSendQuery,
}

struct HecAckClient {
    acks: HashMap<u64, (u8, Sender<EventStatus>)>,
    retry_limit: u8,
    client: HttpClient,
    http_request_builder: Arc<HttpRequestBuilder>,
}

impl HecAckClient {
    fn new(
        retry_limit: u8,
        client: HttpClient,
        http_request_builder: Arc<HttpRequestBuilder>,
    ) -> Self {
        Self {
            acks: HashMap::new(),
            retry_limit,
            client,
            http_request_builder,
        }
    }

    /// Adds an ack id to be queried
    fn add(&mut self, ack_id: u64, ack_event_status_sender: Sender<EventStatus>) {
        self.acks
            .insert(ack_id, (self.retry_limit, ack_event_status_sender));
    }

    /// Queries Splunk HEC with stored ack ids and finalizes events that are successfully acked
    async fn run(&mut self) {
        let ack_query_body = self.get_ack_query_body();
        if !ack_query_body.acks.is_empty() {
            let ack_query_response = self.send_ack_query_request(&ack_query_body).await;

            match ack_query_response {
                Ok(ack_query_response) => {
                    debug!("Received ack statuses {:?}", ack_query_response);
                    let acked_ack_ids = ack_query_response
                        .acks
                        .iter()
                        .filter_map(|(ack_id, ack_status)| ack_status.then(|| *ack_id))
                        .collect::<Vec<u64>>();
                    self.finalize_ack_ids(acked_ack_ids.as_slice());
                }
                Err(error) => {
                    match error {
                        HecAckApiError::ClientParseResponse | HecAckApiError::ClientSendQuery => {
                            // If we are permanently unable to interact with
                            // Splunk HEC indexer acknowledgements (e.g. due to
                            // request/response format changes in future
                            // versions), log an error and fall back to default
                            // behavior.
                            error!(message = "Unable to parse ack query response. Acknowledging based on initial 200 OK.");
                            self.finalize_ack_ids(
                                self.acks.keys().copied().collect::<Vec<_>>().as_slice(),
                            )
                        }
                        _ => {
                            // Otherwise, errors may be recoverable, log an error
                            // and proceed with retries.
                            error!(message = "Unable to send ack query request");
                        }
                    }
                }
            };
        }
    }

    /// Removes successfully acked ack ids and finalizes associated events
    fn finalize_ack_ids(&mut self, ack_ids: &[u64]) {
        for ack_id in ack_ids {
            if let Some((_, ack_event_status_sender)) = self.acks.remove(ack_id) {
                let _ = ack_event_status_sender.send(EventStatus::Delivered);
                debug!("Finalized ack id {:?}", ack_id);
            }
        }
    }

    /// Builds an ack query body with stored ack ids
    fn get_ack_query_body(&mut self) -> HecAckStatusRequest {
        self.clear_expired_ack_ids();
        HecAckStatusRequest {
            acks: self.acks.keys().copied().collect::<Vec<u64>>(),
        }
    }

    /// Decrements retry count on all stored ack ids by 1
    fn decrement_retries(&mut self) {
        for (retries, _) in self.acks.values_mut() {
            *retries = retries.checked_sub(1).unwrap_or(0);
        }
    }

    /// Removes all expired ack ids (those with a retry count of 0).
    fn clear_expired_ack_ids(&mut self) {
        self.acks.retain(|_, (retries, _)| *retries > 0);
    }

    // Sends an ack status query request to Splunk HEC
    async fn send_ack_query_request(
        &mut self,
        request_body: &HecAckStatusRequest,
    ) -> Result<HecAckStatusResponse, HecAckApiError> {
        self.decrement_retries();
        let request_body_bytes =
            serde_json::to_vec(request_body).map_err(|_| HecAckApiError::ClientBuildRequest)?;
        let request = self
            .http_request_builder
            .build_request(request_body_bytes, "/services/collector/ack")
            .map_err(|_| HecAckApiError::ClientBuildRequest)?;

        let response = self
            .client
            .send(request.map(Body::from))
            .await
            .map_err(|_| HecAckApiError::ServerSendQuery)?;

        let status = response.status();
        if status.is_success() {
            let response_body = hyper::body::to_bytes(response.into_body())
                .await
                .map_err(|_| HecAckApiError::ClientParseResponse)?;
            serde_json::from_slice::<HecAckStatusResponse>(&response_body)
                .map_err(|_| HecAckApiError::ClientParseResponse)
        } else if status.is_client_error() {
            Err(HecAckApiError::ClientSendQuery)
        } else {
            Err(HecAckApiError::ServerSendQuery)
        }
    }
}

pub async fn run_acknowledgements(
    mut receiver: UnboundedReceiver<(u64, Sender<EventStatus>)>,
    client: HttpClient,
    http_request_builder: Arc<HttpRequestBuilder>,
    indexer_acknowledgements: HecClientAcknowledgementsConfig,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(
        indexer_acknowledgements.query_interval.get() as u64,
    ));
    let mut ack_client = HecAckClient::new(
        indexer_acknowledgements.retry_limit.get(),
        client,
        http_request_builder,
    );

    loop {
        tokio::select! {
            _ = interval.tick() => {
                ack_client.run().await;
            },
            ack_info = receiver.recv() => {
                match ack_info {
                    Some((ack_id, tx)) => {
                        ack_client.add(ack_id, tx);
                        debug!("Stored ack id {}", ack_id);
                    },
                    None => break,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use futures_util::{stream::FuturesUnordered, StreamExt};
    use tokio::sync::oneshot::{self, Receiver};
    use vector_core::{config::proxy::ProxyConfig, event::EventStatus};

    use crate::{
        http::HttpClient,
        sinks::{
            splunk_hec::common::{
                acknowledgements::HecAckStatusRequest, service::HttpRequestBuilder,
            },
            util::Compression,
        },
    };

    use super::HecAckClient;

    fn get_ack_client(retry_limit: u8) -> HecAckClient {
        let client = HttpClient::new(None, &ProxyConfig::default()).unwrap();
        let http_request_builder =
            HttpRequestBuilder::new(String::from(""), String::from(""), Compression::default());
        HecAckClient::new(retry_limit, client, Arc::new(http_request_builder))
    }

    fn populate_ack_client(
        ack_client: &mut HecAckClient,
        ack_ids: &[u64],
    ) -> Vec<Receiver<EventStatus>> {
        let mut ack_status_rxs = Vec::new();
        for ack_id in ack_ids {
            let (tx, rx) = oneshot::channel();
            ack_client.add(*ack_id, tx);
            ack_status_rxs.push(rx);
        }
        ack_status_rxs
    }

    #[test]
    fn test_get_ack_query_body() {
        let mut ack_client = get_ack_client(1);
        let ack_ids = (0..100).collect::<Vec<u64>>();
        let _ = populate_ack_client(&mut ack_client, &ack_ids);
        let expected_ack_body = HecAckStatusRequest { acks: ack_ids };

        let mut ack_request_body = ack_client.get_ack_query_body();
        ack_request_body.acks.sort_unstable();
        assert_eq!(expected_ack_body, ack_request_body);
    }

    #[test]
    fn test_decrement_retries() {
        let mut ack_client = get_ack_client(1);
        let ack_ids = (0..100).collect::<Vec<u64>>();
        let _ = populate_ack_client(&mut ack_client, &ack_ids);

        let mut ack_request_body = ack_client.get_ack_query_body();
        ack_request_body.acks.sort_unstable();
        assert_eq!(ack_ids, ack_request_body.acks);
        ack_client.decrement_retries();

        let ack_request_body = ack_client.get_ack_query_body();
        assert!(ack_request_body.acks.is_empty())
    }

    #[tokio::test]
    async fn test_finalize_ack_ids() {
        let mut ack_client = get_ack_client(1);
        let ack_ids = (0..100).collect::<Vec<u64>>();
        let ack_status_rxs = populate_ack_client(&mut ack_client, &ack_ids);

        ack_client.finalize_ack_ids(ack_ids.as_slice());
        let mut statuses = ack_status_rxs.into_iter().collect::<FuturesUnordered<_>>();
        while let Some(status) = statuses.next().await {
            assert_eq!(EventStatus::Delivered, status.unwrap());
        }
    }
}
