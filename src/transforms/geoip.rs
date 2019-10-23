use super::Transform;

use crate::{
    event::Event,
    topology::config::{DataType, TransformConfig},
};
use serde::{Deserialize, Serialize};
use string_cache::DefaultAtom as Atom;

use indexmap::IndexMap;
use std::net::IpAddr;
use std::str::FromStr;

#[derive(Deserialize, Serialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct GeoipConfig {
    pub source: Atom,
    pub database: String,
    pub target: String,
}

pub struct Geoip {
    pub dbreader: maxminddb::Reader<Vec<u8>>,
    pub source: Atom,
    pub target: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct GeoipDecodedData {
    pub data: IndexMap<Atom, String>,
}

#[typetag::serde(name = "geoip")]
impl TransformConfig for GeoipConfig {
    fn build(&self) -> Result<Box<dyn Transform>, crate::Error> {
        let reader = maxminddb::Reader::open_readfile(self.database.clone()).unwrap();
        Ok(Box::new(Geoip::new(
            reader,
            self.source.clone(),
            self.target.clone(),
        )))
    }

    fn input_type(&self) -> DataType {
        DataType::Log
    }

    fn output_type(&self) -> DataType {
        DataType::Log
    }
}

impl Geoip {
    pub fn new(dbreader: maxminddb::Reader<Vec<u8>>, source: Atom, target: String) -> Self {
        Geoip {
            dbreader: dbreader,
            source: source,
            target: target,
        }
    }
}

impl Transform for Geoip {
    fn transform(&mut self, mut event: Event) -> Option<Event> {
        println!("Event: {:?}", event.as_log());
        let ipaddress = event
            .as_log()
            .get(&self.source)
            .map(|s| s.to_string_lossy());
        if let Some(ipaddress) = &ipaddress {
            let ip: IpAddr = FromStr::from_str(ipaddress).unwrap();
            println!("Looking up {}", ip);
            let v = self.dbreader.lookup(ip);
            if v.is_ok() {
                let city: maxminddb::geoip2::City = v.unwrap();
                let iso_code = city.country.and_then(|cy| cy.iso_code);
                if let Some(iso_code) = iso_code {
                    let mut d = IndexMap::new();
                    d.insert(Atom::from("city"), iso_code.into());
                    let geoipdata = GeoipDecodedData { data: d };
                    event.as_mut_log().insert_explicit(
                        Atom::from(self.target.clone()),
                        serde_json::to_string(&geoipdata).unwrap().into(),
                    );
                }
            }
        } else {
            println!("Something went wrong: {:?}", Some(ipaddress));
            debug!(
                message = "Field does not exist.",
                field = self.source.as_ref(),
            );
        };

        Some(event)
    }
}

#[cfg(test)]
mod tests {
    use super::Geoip;
    use crate::{
        event::Event,
        transforms::json_parser::{JsonParser, JsonParserConfig},
        transforms::Transform,
    };
    use string_cache::DefaultAtom as Atom;

    #[test]
    fn geoip_event() {
        let mut parser = JsonParser::from(JsonParserConfig::default());
        let event = Event::from(r#"{"remote_addr": "8.8.8.8", "request_path": "foo/bar"}"#);
        let event = parser.transform(event).unwrap();
        let reader = maxminddb::Reader::open_readfile("test-data/GeoIP2-City-Test.mmdb").unwrap();

        let mut augment = Geoip::new(reader, Atom::from("remote_addr"), "geoip".to_string());
        let new_event = augment.transform(event).unwrap();
        println!("Event after transformation: {:?}", new_event.as_log());

        // we expect there will be no geoip object in the transformed
        // event
    }
}
