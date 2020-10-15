use super::Transform;
use crate::{
    config::{DataType, TransformConfig, TransformContext, TransformDescription},
    event::Event,
    sinks::util::http::HttpClient,
};
use bytes::Bytes;
use http::{uri::PathAndQuery, Request, StatusCode, Uri};
use hyper::{body::to_bytes as body_to_bytes, Body};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::RandomState, HashSet};

use tokio::time::{delay_for, Duration, Instant};
use tracing_futures::Instrument;

type WriteHandle = evmap::WriteHandle<String, Bytes, (), RandomState>;
type ReadHandle = evmap::ReadHandle<String, Bytes, (), RandomState>;

const AMI_ID_KEY: &str = "ami-id";
const AVAILABILITY_ZONE_KEY: &str = "availability-zone";
const INSTANCE_ID_KEY: &str = "instance-id";
const INSTANCE_TYPE_KEY: &str = "instance-type";
const LOCAL_HOSTNAME_KEY: &str = "local-hostname";
const LOCAL_IPV4_KEY: &str = "local-ipv4";
const PUBLIC_HOSTNAME_KEY: &str = "public-hostname";
const PUBLIC_IPV4_KEY: &str = "public-ipv4";
const REGION_KEY: &str = "region";
const SUBNET_ID_KEY: &str = "subnet-id";
const VPC_ID_KEY: &str = "vpc-id";
const ROLE_NAME_KEY: &str = "role-name";

lazy_static::lazy_static! {
    static ref AMI_ID: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/ami-id");
    static ref AVAILABILITY_ZONE: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/placement/availability-zone");
    static ref INSTANCE_ID: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/instance-id");
    static ref INSTANCE_TYPE: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/instance-type");
    static ref LOCAL_HOSTNAME: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/local-hostname");
    static ref LOCAL_IPV4: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/local-ipv4");
    static ref PUBLIC_HOSTNAME: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/public-hostname");
    static ref PUBLIC_IPV4: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/public-ipv4");
    static ref REGION: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/region");
    static ref SUBNET_ID: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/network/interfaces/macs/mac/subnet-id");
    static ref VPC_ID: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/network/interfaces/macs/mac/vpc-id");
    static ref ROLE_NAME: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/iam/security-credentials/");
    static ref MAC: PathAndQuery = PathAndQuery::from_static("/latest/meta-data/mac");
    static ref DYNAMIC_DOCUMENT: PathAndQuery = PathAndQuery::from_static("/latest/dynamic/instance-identity/document");
    static ref DEFAULT_FIELD_WHITELIST: Vec<String> = vec![
        AMI_ID_KEY.to_string(),
        AVAILABILITY_ZONE_KEY.to_string(),
        INSTANCE_ID_KEY.to_string(),
        INSTANCE_TYPE_KEY.to_string(),
        LOCAL_HOSTNAME_KEY.to_string(),
        LOCAL_IPV4_KEY.to_string(),
        PUBLIC_HOSTNAME_KEY.to_string(),
        PUBLIC_IPV4_KEY.to_string(),
        REGION_KEY.to_string(),
        SUBNET_ID_KEY.to_string(),
        VPC_ID_KEY.to_string(),
        ROLE_NAME_KEY.to_string(),
    ];
    static ref API_TOKEN: PathAndQuery = PathAndQuery::from_static("/latest/api/token");
    static ref TOKEN_HEADER: Bytes = Bytes::from("X-aws-ec2-metadata-token");
    static ref HOST: Uri = Uri::from_static("http://169.254.169.254");
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Ec2Metadata {
    // Deprecated name
    #[serde(alias = "host")]
    endpoint: Option<String>,
    namespace: Option<String>,
    refresh_interval_secs: Option<u64>,
    fields: Option<Vec<String>>,
}

pub struct Ec2MetadataTransform {
    state: ReadHandle,
}

#[derive(Debug, Clone)]
struct Keys {
    ami_id_key: String,
    availability_zone_key: String,
    instance_id_key: String,
    instance_type_key: String,
    local_hostname_key: String,
    local_ipv4_key: String,
    public_hostname_key: String,
    public_ipv4_key: String,
    region_key: String,
    subnet_id_key: String,
    vpc_id_key: String,
    role_name_key: String,
}

inventory::submit! {
    TransformDescription::new::<Ec2Metadata>("aws_ec2_metadata")
}

impl_generate_config_from_default!(Ec2Metadata);

#[async_trait::async_trait]
#[typetag::serde(name = "aws_ec2_metadata")]
impl TransformConfig for Ec2Metadata {
    async fn build(&self, cx: TransformContext) -> crate::Result<Box<dyn Transform>> {
        let (read, write) = evmap::new();

        // Check if the namespace is set to `""` which should mean that we do
        // not want a prefixed namespace.
        let namespace = self.namespace.clone().and_then(|namespace| {
            if namespace == "" {
                None
            } else {
                Some(namespace)
            }
        });

        let keys = Keys::new(&namespace);

        let host = self
            .endpoint
            .clone()
            .map(|s| Uri::from_maybe_shared(s).unwrap())
            .unwrap_or_else(|| HOST.clone());

        let refresh_interval = self
            .refresh_interval_secs
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(10));
        let fields = self
            .fields
            .clone()
            .unwrap_or_else(|| DEFAULT_FIELD_WHITELIST.clone());

        let http_client = HttpClient::new(cx.resolver(), None)?;

        let mut client =
            MetadataClient::new(http_client, host, keys, write, refresh_interval, fields);

        client.refresh_metadata().await?;

        tokio::spawn(
            async move {
                client.run().await;
            }
            // TODO: Once #1338 is done we can fetch the current span
            .instrument(info_span!("aws_ec2_metadata: worker")),
        );

        Ok(Box::new(Ec2MetadataTransform { state: read }))
    }

    fn input_type(&self) -> DataType {
        DataType::Log
    }

    fn output_type(&self) -> DataType {
        DataType::Log
    }

    fn transform_type(&self) -> &'static str {
        "aws_ec2_metadata"
    }
}

impl Transform for Ec2MetadataTransform {
    fn transform(&mut self, mut event: Event) -> Option<Event> {
        let log = event.as_mut_log();

        if let Some(read_ref) = self.state.read() {
            read_ref.into_iter().for_each(|(k, v)| {
                if let Some(value) = v.get_one() {
                    log.insert(k.clone(), value.clone());
                }
            });
        }

        Some(event)
    }
}

struct MetadataClient {
    client: HttpClient<Body>,
    host: Uri,
    token: Option<(Bytes, Instant)>,
    keys: Keys,
    state: WriteHandle,
    refresh_interval: Duration,
    fields: HashSet<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdentityDocument {
    account_id: String,
    architecture: String,
    image_id: String,
    instance_id: String,
    instance_type: String,
    private_ip: String,
    region: String,
    version: String,
}

impl MetadataClient {
    pub fn new(
        client: HttpClient<Body>,
        host: Uri,
        keys: Keys,
        state: WriteHandle,
        refresh_interval: Duration,
        fields: Vec<String>,
    ) -> Self {
        Self {
            client,
            host,
            token: None,
            keys,
            state,
            refresh_interval,
            fields: fields.into_iter().collect(),
        }
    }

    async fn run(&mut self) {
        loop {
            if let Err(error) = self.refresh_metadata().await {
                error!(message = "Unable to fetch EC2 metadata; Retrying.", %error);
                delay_for(Duration::from_secs(1)).await;
                continue;
            }

            delay_for(self.refresh_interval).await;
        }
    }

    pub async fn get_token(&mut self) -> Result<Bytes, crate::Error> {
        if let Some((token, next_refresh)) = self.token.clone() {
            // If the next refresh is greater (in the future) than
            // the current time we can return the token since its still valid
            // otherwise lets refresh it.
            if next_refresh > Instant::now() {
                return Ok(token);
            }
        }

        let mut parts = self.host.clone().into_parts();
        parts.path_and_query = Some(API_TOKEN.clone());
        let uri = Uri::from_parts(parts)?;

        let req = Request::put(uri)
            .header("X-aws-ec2-metadata-token-ttl-seconds", "21600")
            .body(Body::empty())?;

        let res = self.client.send(req).await?;

        if res.status() != StatusCode::OK {
            return Err(Ec2MetadataError::UnableToFetchToken.into());
        }

        let token = body_to_bytes(res.into_body()).await?;

        let next_refresh = Instant::now() + Duration::from_secs(21600);
        self.token = Some((token.clone(), next_refresh));

        Ok(token)
    }

    pub async fn get_document(&mut self) -> Result<Option<IdentityDocument>, crate::Error> {
        let token = self.get_token().await?;

        let mut parts = self.host.clone().into_parts();
        parts.path_and_query = Some(DYNAMIC_DOCUMENT.clone());
        let uri = Uri::from_parts(parts)?;

        let req = Request::get(uri)
            .header(TOKEN_HEADER.as_ref(), token.as_ref())
            .body(Body::empty())?;

        let res = self.client.send(req).await?;

        if res.status() != StatusCode::OK {
            warn!(message="Identity document request failed.", status = %res.status());
            return Ok(None);
        }

        let body = body_to_bytes(res.into_body()).await?;

        serde_json::from_slice(&body[..])
            .map_err(Into::into)
            .map(Some)
    }

    pub async fn get_metadata(
        &mut self,
        path: &PathAndQuery,
    ) -> Result<Option<Bytes>, crate::Error> {
        let token = self.get_token().await?;

        let mut parts = self.host.clone().into_parts();

        parts.path_and_query = Some(path.clone());

        let uri = Uri::from_parts(parts)?;

        debug!(message = "Sending metadata request.", %uri);

        let req = Request::get(uri)
            .header(TOKEN_HEADER.as_ref(), token.as_ref())
            .body(Body::empty())?;

        let res = self.client.send(req).await?;

        if StatusCode::OK != res.status() {
            warn!(message="Metadata request failed.", status = %res.status());
            return Ok(None);
        }

        let body = body_to_bytes(res.into_body()).await?;

        Ok(Some(body))
    }

    pub async fn refresh_metadata(&mut self) -> Result<(), crate::Error> {
        // Fetch all resources, _then_ add them to the state map.
        let identity_document = self.get_document().await?;

        if let Some(document) = identity_document {
            if self.fields.contains(&*AMI_ID_KEY) {
                self.state
                    .update(self.keys.ami_id_key.clone(), document.image_id.into());
            }

            if self.fields.contains(&*INSTANCE_ID_KEY) {
                self.state.update(
                    self.keys.instance_id_key.clone(),
                    document.instance_id.into(),
                );
            }

            if self.fields.contains(&*INSTANCE_TYPE_KEY) {
                self.state.update(
                    self.keys.instance_type_key.clone(),
                    document.instance_type.into(),
                );
            }

            if self.fields.contains(&*REGION_KEY) {
                self.state
                    .update(self.keys.region_key.clone(), document.region.into());
            }
        }

        if self.fields.contains(&*AVAILABILITY_ZONE_KEY) {
            if let Some(availability_zone) = self.get_metadata(&AVAILABILITY_ZONE).await? {
                self.state
                    .update(self.keys.availability_zone_key.clone(), availability_zone);
            }
        }

        if self.fields.contains(&*LOCAL_HOSTNAME_KEY) {
            if let Some(local_hostname) = self.get_metadata(&LOCAL_HOSTNAME).await? {
                self.state
                    .update(self.keys.local_hostname_key.clone(), local_hostname);
            }
        }

        if self.fields.contains(&*LOCAL_IPV4_KEY) {
            if let Some(local_ipv4) = self.get_metadata(&LOCAL_IPV4).await? {
                self.state
                    .update(self.keys.local_ipv4_key.clone(), local_ipv4);
            }
        }

        if self.fields.contains(&*PUBLIC_HOSTNAME_KEY) {
            if let Some(public_hostname) = self.get_metadata(&PUBLIC_HOSTNAME).await? {
                self.state
                    .update(self.keys.public_hostname_key.clone(), public_hostname);
            }
        }

        if self.fields.contains(&*PUBLIC_IPV4_KEY) {
            if let Some(public_ipv4) = self.get_metadata(&PUBLIC_IPV4).await? {
                self.state
                    .update(self.keys.public_ipv4_key.clone(), public_ipv4);
            }
        }

        if self.fields.contains(&*SUBNET_ID_KEY) || self.fields.contains(VPC_ID_KEY) {
            if let Some(mac) = self.get_metadata(&MAC).await? {
                let mac = String::from_utf8_lossy(&mac[..]);

                let subnet_path = format!(
                    "/latest/meta-data/network/interfaces/macs/{}/subnet-id",
                    mac
                )
                .parse()?;
                let vpc_path =
                    format!("/latest/meta-data/network/interfaces/macs/{}/vpc-id", mac).parse()?;

                if self.fields.contains(&*SUBNET_ID_KEY) {
                    if let Some(subnet_id) = self.get_metadata(&subnet_path).await? {
                        self.state
                            .update(self.keys.subnet_id_key.clone(), subnet_id);
                    }
                }

                if self.fields.contains(&*VPC_ID_KEY) {
                    if let Some(vpc_id) = self.get_metadata(&vpc_path).await? {
                        self.state.update(self.keys.vpc_id_key.clone(), vpc_id);
                    }
                }
            }
        }

        if self.fields.contains(&*ROLE_NAME_KEY) {
            if let Some(role_names) = self.get_metadata(&ROLE_NAME).await? {
                let role_names = String::from_utf8_lossy(&role_names[..]);

                for (i, role_name) in role_names.lines().enumerate() {
                    self.state.update(
                        format!("{}[{}]", self.keys.role_name_key, i),
                        role_name.to_string().into(),
                    );
                }
            }
        }

        // Make changes viewable to the transform. This may block if
        // readers are still reading.
        self.state.refresh();

        Ok(())
    }
}

impl Keys {
    pub fn new(namespace: &Option<String>) -> Self {
        if let Some(namespace) = &namespace {
            Keys {
                ami_id_key: format!("{}.{}", namespace, AMI_ID_KEY),
                availability_zone_key: format!("{}.{}", namespace, AVAILABILITY_ZONE_KEY),
                instance_id_key: format!("{}.{}", namespace, INSTANCE_ID_KEY),
                instance_type_key: format!("{}.{}", namespace, INSTANCE_TYPE_KEY),
                local_hostname_key: format!("{}.{}", namespace, LOCAL_HOSTNAME_KEY),
                local_ipv4_key: format!("{}.{}", namespace, LOCAL_IPV4_KEY),
                public_hostname_key: format!("{}.{}", namespace, PUBLIC_HOSTNAME_KEY),
                public_ipv4_key: format!("{}.{}", namespace, PUBLIC_IPV4_KEY),
                region_key: format!("{}.{}", namespace, REGION_KEY),
                subnet_id_key: format!("{}.{}", namespace, SUBNET_ID_KEY),
                vpc_id_key: format!("{}.{}", namespace, VPC_ID_KEY),
                role_name_key: format!("{}.{}", namespace, VPC_ID_KEY),
            }
        } else {
            Keys {
                ami_id_key: AMI_ID_KEY.into(),
                availability_zone_key: AVAILABILITY_ZONE_KEY.into(),
                instance_id_key: INSTANCE_ID_KEY.into(),
                instance_type_key: INSTANCE_TYPE_KEY.into(),
                local_hostname_key: LOCAL_HOSTNAME_KEY.into(),
                local_ipv4_key: LOCAL_IPV4_KEY.into(),
                public_hostname_key: PUBLIC_HOSTNAME_KEY.into(),
                public_ipv4_key: PUBLIC_IPV4_KEY.into(),
                region_key: REGION_KEY.into(),
                subnet_id_key: SUBNET_ID_KEY.into(),
                vpc_id_key: VPC_ID_KEY.into(),
                role_name_key: ROLE_NAME_KEY.into(),
            }
        }
    }
}

#[derive(Debug, snafu::Snafu)]
enum Ec2MetadataError {
    #[snafu(display("Unable to fetch token."))]
    UnableToFetchToken,
}

#[cfg(feature = "aws-ec2-metadata-integration-tests")]
#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::{event::Event, test_util::trace_init};

    const HOST: &str = "http://localhost:8111";

    #[test]
    fn generate_config() {
        crate::test_util::test_generate_config::<Ec2Metadata>();
    }

    #[tokio::test]
    async fn enrich() {
        trace_init();

        let config = Ec2Metadata {
            endpoint: Some(HOST.to_string()),
            ..Default::default()
        };
        let mut transform = config.build(TransformContext::new_test()).await.unwrap();

        // We need to sleep to let the background task fetch the data.
        delay_for(Duration::from_secs(1)).await;

        let event = Event::new_empty_log();

        let event = transform.transform(event).unwrap();
        let log = event.as_log();

        assert_eq!(log.get("availability-zone"), Some(&"ww-region-1a".into()));
        assert_eq!(log.get("public-ipv4"), Some(&"192.1.1.1".into()));
        assert_eq!(
            log.get("public-hostname"),
            Some(&"mock-public-hostname".into())
        );
        assert_eq!(log.get(&"local-ipv4"), Some(&"192.1.1.2".into()));
        assert_eq!(log.get("local-hostname"), Some(&"mock-hostname".into()));
        assert_eq!(log.get("instance-id"), Some(&"i-096fba6d03d36d262".into()));
        assert_eq!(log.get("ami-id"), Some(&"ami-05f27d4d6770a43d2".into()));
        assert_eq!(log.get("instance-type"), Some(&"t2.micro".into()));
        assert_eq!(log.get("region"), Some(&"us-east-1".into()));
        assert_eq!(log.get("vpc-id"), Some(&"mock-vpc-id".into()));
        assert_eq!(log.get("subnet-id"), Some(&"mock-subnet-id".into()));
        assert_eq!(log.get("role-name[0]"), Some(&"mock-user".into()));
    }

    #[tokio::test]
    async fn fields() {
        let config = Ec2Metadata {
            endpoint: Some(HOST.to_string()),
            fields: Some(vec!["public-ipv4".into(), "region".into()]),
            ..Default::default()
        };
        let mut transform = config.build(TransformContext::new_test()).await.unwrap();

        // We need to sleep to let the background task fetch the data.
        delay_for(Duration::from_secs(1)).await;

        let event = Event::new_empty_log();

        let event = transform.transform(event).unwrap();
        let log = event.as_log();

        assert_eq!(log.get("availability-zone"), None);
        assert_eq!(log.get("public-ipv4"), Some(&"192.1.1.1".into()));
        assert_eq!(log.get("public-hostname"), None);
        assert_eq!(log.get("local-ipv4"), None);
        assert_eq!(log.get("local-hostname"), None);
        assert_eq!(log.get("instance-id"), None,);
        assert_eq!(log.get("instance-type"), None,);
        assert_eq!(log.get("ami-id"), None);
        assert_eq!(log.get("region"), Some(&"us-east-1".into()));
    }

    #[tokio::test]
    async fn namespace() {
        let config = Ec2Metadata {
            endpoint: Some(HOST.to_string()),
            namespace: Some("ec2.metadata".into()),
            ..Default::default()
        };
        let mut transform = config.build(TransformContext::new_test()).await.unwrap();

        // We need to sleep to let the background task fetch the data.
        delay_for(Duration::from_secs(1)).await;

        let event = Event::new_empty_log();

        let event = transform.transform(event).unwrap();
        let log = event.as_log();

        assert_eq!(
            log.get("ec2.metadata.availability-zone"),
            Some(&"ww-region-1a".into())
        );
        assert_eq!(
            log.get("ec2.metadata.public-ipv4"),
            Some(&"192.1.1.1".into())
        );

        // Set an empty namespace to ensure we don't prepend one.
        let config = Ec2Metadata {
            endpoint: Some(HOST.to_string()),
            namespace: Some("".into()),
            ..Default::default()
        };
        let mut transform = config.build(TransformContext::new_test()).await.unwrap();

        // We need to sleep to let the background task fetch the data.
        delay_for(Duration::from_secs(1)).await;

        let event = Event::new_empty_log();

        let event = transform.transform(event).unwrap();
        let log = event.as_log();

        assert_eq!(log.get("availability-zone"), Some(&"ww-region-1a".into()));
        assert_eq!(log.get("public-ipv4"), Some(&"192.1.1.1".into()));
    }
}
