#[readonly::make]
#[derive(Default, Debug, Clone)]
pub struct DnstapEventSchema {
    pub dnstap_root_data_schema: DnstapRootDataSchema,
    pub dnstap_message_schema: DnstapMessageSchema,
    pub dns_query_message_schema: DnsQueryMessageSchema,
    pub dns_query_header_schema: DnsQueryHeaderSchema,
    pub dns_message_opt_pseudo_section_schema: DnsMessageOptPseudoSectionSchema,
    pub dns_message_option_schema: DnsMessageOptionSchema,
    pub dns_record_schema: DnsRecordSchema,
}

impl DnstapEventSchema {
    pub fn new() -> Self {
        Self {
            dnstap_root_data_schema: DnstapRootDataSchema::default(),
            dnstap_message_schema: DnstapMessageSchema::default(),
            dns_query_message_schema: DnsQueryMessageSchema::default(),
            dns_query_header_schema: DnsQueryHeaderSchema::default(),
            dns_message_opt_pseudo_section_schema: DnsMessageOptPseudoSectionSchema::default(),
            dns_message_option_schema: DnsMessageOptionSchema::default(),
            dns_record_schema: DnsRecordSchema::default(),
        }
    }
}

#[readonly::make]
#[derive(Debug, Clone)]
pub struct DnstapRootDataSchema {
    pub server_identity: String,
    pub server_version: String,
    pub extra: String,
    pub data_type: String,
    pub timestamp: String,
    pub time_precision: String,
    pub error: String,
    pub raw_data: String,
}

impl Default for DnstapRootDataSchema {
    fn default() -> Self {
        Self {
            server_identity: String::from("serverId"),
            server_version: String::from("serverVersion"),
            extra: String::from("extraInfo"),
            data_type: String::from("type"),
            timestamp: String::from("time"),
            time_precision: String::from("time_precision"),
            error: String::from("error"),
            raw_data: String::from("rawData"),
        }
    }
}

#[readonly::make]
#[derive(Debug, Clone)]
pub struct DnstapMessageSchema {
    pub socket_family: String,
    pub socket_protocol: String,
    pub query_address: String,
    pub query_port: String,
    pub response_address: String,
    pub response_port: String,
    pub query_zone: String,
    pub dnstap_message_type: String,
    pub query_message: String,
    pub response_message: String,
}

impl Default for DnstapMessageSchema {
    fn default() -> Self {
        Self {
            socket_family: String::from("data.socketFamily"),
            socket_protocol: String::from("data.socketProtocol"),
            query_address: String::from("data.sourceAddress"),
            query_port: String::from("data.sourcePport"),
            response_address: String::from("data.responseAddress"),
            response_port: String::from("data.responsePort"),
            query_zone: String::from("data.queryZone"),
            dnstap_message_type: String::from("data.type"),
            query_message: String::from("data.requestData"),
            response_message: String::from("data.responseData"),
        }
    }
}

#[readonly::make]
#[derive(Debug, Clone)]
pub struct DnsQueryMessageSchema {
    pub response_code: String,
    pub response: String,
    pub timestamp: String,
    pub time_precision: String,
    pub raw_data: String,
    pub header: String,
    pub question_section: String,
    pub answer_section: String,
    pub authority_section: String,
    pub additional_section: String,
    pub opt_pseudo_section: String,
}

impl Default for DnsQueryMessageSchema {
    fn default() -> Self {
        Self {
            response_code: String::from("responseCode"),
            response: String::from("rcodeName"),
            timestamp: String::from("time"),
            time_precision: String::from("timePrecision"),
            raw_data: String::from("rawData"),
            header: String::from("header"),
            question_section: String::from("question"),
            answer_section: String::from("answers"),
            authority_section: String::from("authority"),
            additional_section: String::from("additional"),
            opt_pseudo_section: String::from("opt"),
        }
    }
}

#[readonly::make]
#[derive(Debug, Clone)]
pub struct DnsQueryHeaderSchema {
    pub id: String,
    pub opcode: String,
    pub rcode: String,
    pub qr: String,
    pub aa: String,
    pub tc: String,
    pub rd: String,
    pub ra: String,
    pub ad: String,
    pub cd: String,
    pub question_count: String,
    pub answer_count: String,
    pub authority_count: String,
    pub additional_count: String,
}

impl Default for DnsQueryHeaderSchema {
    fn default() -> Self {
        Self {
            id: String::from("id"),
            opcode: String::from("opcode"),
            rcode: String::from("rcode"),
            qr: String::from("qr"),
            aa: String::from("aa"),
            tc: String::from("tc"),
            rd: String::from("rd"),
            ra: String::from("ra"),
            ad: String::from("ad"),
            cd: String::from("cd"),
            question_count: String::from("qdCount"),
            answer_count: String::from("anCount"),
            authority_count: String::from("nsCount"),
            additional_count: String::from("arCount"),
        }
    }
}

#[readonly::make]
#[derive(Debug, Clone)]
pub struct DnsMessageOptPseudoSectionSchema {
    pub extended_rcode: String,
    pub version: String,
    pub do_flag: String,
    pub udp_max_payload_size: String,
    pub options: String,
}

impl Default for DnsMessageOptPseudoSectionSchema {
    fn default() -> Self {
        Self {
            extended_rcode: String::from("extendedRcode"),
            version: String::from("ednsVersion"),
            do_flag: String::from("do"),
            udp_max_payload_size: String::from("udpPayloadSize"),
            options: String::from("options"),
        }
    }
}

#[readonly::make]
#[derive(Debug, Clone)]
pub struct DnsMessageOptionSchema {
    pub opt_code: String,
    pub opt_name: String,
    pub opt_data: String,
}

impl Default for DnsMessageOptionSchema {
    fn default() -> Self {
        Self {
            opt_code: String::from("optCode"),
            opt_name: String::from("optName"),
            opt_data: String::from("optValue"),
        }
    }
}

#[readonly::make]
#[derive(Debug, Clone)]
pub struct DnsRecordSchema {
    pub name: String,
    pub record_type: String,
    pub record_type_id: String,
    pub ttl: String,
    pub class: String,
    pub rdata: String,
    pub rdata_bytes: String,
}

impl Default for DnsRecordSchema {
    fn default() -> Self {
        Self {
            name: String::from("domainName"),
            record_type: String::from("recordType"),
            record_type_id: String::from("recordTypeId"),
            ttl: String::from("ttl"),
            class: String::from("class"),
            rdata: String::from("rData"),
            rdata_bytes: String::from("rDataBytes"),
        }
    }
}
