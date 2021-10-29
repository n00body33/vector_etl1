package metadata

import "strings"

_values: {
	current_timestamp: "2020-10-10T17:07:36.452332Z"
	local_host:        "my-host.local"
	remote_host:       "34.33.222.212"
	instance:          "vector:9598"
}

// `#Any` allows for any value.
#Any: *_ | {[_=string]: #Any}

#Arch: "ARM64" | "ARMv7" | "x86_64"

// `#CompressionAlgorithm` specified data compression algorithm.
//
// * `none` - compression is not applied
// * `gzip` - gzip compression applied
#CompressionAlgorithm: "none" | "gzip" | "lz4" | "snappy" | "zstd"

#CompressionLevel: "none" | "fast" | "default" | "best" | >=0 & <=9

#Date: =~"^\\d{4}-\\d{2}-\\d{2}"

// `#DeliveryStatus` documents the delivery guarantee.
//
// * `at_least_once` - The event will be delivered at least once and
// could be delivered more than once.
// * `best_effort` - We will make a best effort to deliver the event,
// but the event is not guaranteed to be delivered.
#DeliveryStatus: "at_least_once" | "best_effort"

// `#DeploymentRoles` clarify when a component should be used under
// certain deployment contexts.
//
// * `daemon` - Vector is installed as a single process on the host.
// * `sidecar` - Vector is installed alongside each process it is
//   monitoring. Therefore, there might be multiple Vector processes
//   on the host.
// * `aggregator` - Vector receives data from one or more upstream
//   sources, typically over a network protocol.
#DeploymentRole: "aggregator" | "daemon" | "sidecar"

// `#DevelopmentStatus` documents the development status of the component.
//
// * `beta` - The component is early in its development cycle and the
// API and reliability are not settled.
// * `stable` - The component is production ready.
// * `deprecated` - The component will be removed in a future version.
// * `removed` - The component has been removed.
#DevelopmentStatus: "beta" | "stable" | "deprecated" | "removed"

#EncodingCodec: "json" | "logfmt" | "ndjson" | "text"

#Endpoint: {
	description: string
	responses: [Code=string]: {
		description: string
	}
}

#Endpoints: [Path=string]: {
	DELETE?: #Endpoint
	GET?:    #Endpoint
	POST?:   #Endpoint
	PUT?:    #Endpoint
}

// `enum` restricts the value to a set of values.
//
//                enum: {
//                 json: "Encodes the data via application/json"
//                 text: "Encodes the data via text/plain"
//                }
#Enum: [Name=_]: string

#Event: {
	{log?: #LogEvent} |
	{metric?: #MetricEvent}
}

// `#EventType` represents one of Vector's supported event types.
//
// * `log` - log event
// * `metric` - metric event
#EventType: "log" | "metric"

#Fields: [Name=string]: #Fields | *_

#Interface: {
	{binary: #InterfaceBinary} |
	{ffi: {}} |
	{file_system: #InterfaceFileSystem} |
	{socket: #InterfaceSocket} |
	{stdin: {}} |
	{stdout: {}}
}

#InterfaceBinary: {
	name:         string
	permissions?: #Permissions
}

#InterfaceFileSystem: {
	directory: string
}

#InterfaceSocket: {
	api?: {
		title: string
		url:   string
	}
	direction: "incoming" | "outgoing"

	if direction == "outgoing" {
		network_hops?: uint8
		permissions?:  #Permissions
	}

	if direction == "incoming" {
		port: uint16
	}

	protocols: [#Protocol, ...#Protocol]
	socket?: string
	ssl:     "disabled" | "required" | "optional"
}

#HowItWorks: [Name=string]: {
	#Subsection: {
		title: string
		body:  string
	}

	name:  Name
	title: string
	body:  string | null
	svg?:  string
	sub_sections?: [#Subsection, ...#Subsection]
}

#LogEvent: {
	...
}

#Map: [string]: string

#MetricEvent: {
	kind:       "incremental" | "absolute"
	name:       string
	namespace?: string
	tags: [Name=string]: string
	timestamp?: string
	{counter: #MetricEventCounter} |
	{distribution: #MetricEventDistribution} |
	{gauge: #MetricEventGauge} |
	{histogram: #MetricEventHistogram} |
	{set: #MetricEventSet} |
	{summary: #MetricEventSummary}
}

#MetricEventCounter: {
	value: float
}

#MetricEventDistribution: {
	samples: [#DistributionSample, ...#DistributionSample]
	statistic: "histogram" | "summary"
}

#DistributionSample: {
	value: float
	rate:  uint
}

#MetricEventGauge: {
	value: float
}

#MetricEventHistogram: {
	buckets: [#HistogramBucket, ...#HistogramBucket]
	count: uint
	sum:   float
}

#HistogramBucket: {
	upper_limit: float
	count:       uint
}

#MetricEventSet: {
	values: [string, ...string]
}

#MetricEventSummary: {
	quantiles: [#SummaryQuantile, ...#SummaryQuantile]
	count: int
	sum:   float
}

#SummaryQuantile: {
	upper_limit: float
	value:       float
}

#MetricTags: [Name=string]: {
	name:        Name
	default?:    string
	description: string
	enum?:       #Enum
	examples?: [string, ...string]
	required: bool
}

#MetricType: "counter" | "distribution" | "gauge" | "histogram" | "summary"

#Object: {[_=string]: #Any}

#OperatingSystemFamily: "Linux" | "macOS" | "Windows"

#Permissions: {
	unix: {
		group: string
	}
}

#Protocol: "http" | "tcp" | "udp" | "unix" | "unix_datagram" | "unix_stream"

#Service: {
	// `description` describes the components with a single paragraph.
	// It is used for SEO purposes and should be full of relevant keywords.
	description?: =~"[.]$"

	name:     string
	thing:    string
	url:      string
	versions: string | null

	setup?: #SetupSteps

	connect_to?: [_=string]: {
		logs?: {
			description?: string
			setup:        #SetupSteps
		}

		metrics?: {
			description?: string
			setup:        #SetupSteps
		}
	}
}

#SetupStep: {
	title:        string
	description?: string
	notes?: [...string]

	detour?: {
		url: string
	}

	vector?: {
		configure: #Object
	}

	if detour == _|_ && vector == _|_ {
		description: string
	}
}

#SetupSteps: [#SetupStep, ...#SetupStep]

#Schema: [Name=string]: #SchemaField & {name: Name}

#SchemaField: {
	// `category` allows you to group options into categories.
	//
	// For example, all of the `*_key` options might be grouped under the
	// "Context" category to make generated configuration examples easier to
	// read.
	category?: string

	if type.object != _|_ {
		category: strings.ToTitle(name)
	}

	// `description` describes the option in a succinct fashion. Usually 1 to
	// 2 sentences.
	description: string

	// `groups` groups options into categories.
	//
	// For example, the `influxdb_logs` sink supports both v1 and v2 of Influxdb
	// and relevant options are placed in those groups.
	groups?: [string, ...string]

	// `name` sets the name for this option. It is automatically set for you
	// via the key you use.
	name: string

	// `relevant_when` clarifies when an option is relevant.
	//
	// For example, if an option depends on the value of another option you can
	// specify that here. We accept a string to allow for the expression of
	// complex requirements.
	//
	//              relevant_when: 'strategy = "fingerprint"'
	//              relevant_when: 'strategy = "fingerprint" or "inode"'
	relevant_when?: string

	// `required` requires the option to be set.
	required: bool

	// `warnings` warn the user about some aspects of the option.
	//
	// For example, the `tls.verify_hostname` option has a warning about
	// reduced security if the option is disabled.
	warnings: [...string] | *[]

	if !required {
		// `common` specifes that the option is commonly used. It will bring the
		// option to the top of the documents, surfacing it from other
		// less common, options.
		common: bool
	}

	// `sort` sorts the option, otherwise options will be sorted alphabetically.
	sort?: int8

	// `types` sets the option's value type. External tagging is used since
	// each type has its own set of fields.
	type: #Type & {_args: "required": required}
}

#TargetTriples: {
	"aarch64-unknown-linux-gnu":      bool
	"aarch64-unknown-linux-musl":     bool
	"armv7-unknown-linux-gnueabihf":  bool
	"armv7-unknown-linux-musleabihf": bool
	"x86_64-apple-darwin":            bool
	"x86_64-pc-windows-msv":          bool
	"x86_64-unknown-linux-gnu":       bool
	"x86_64-unknown-linux-musl":      bool
}

#Timestamp: =~"^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}.\\d{6}Z"

#Type: {
	_args: {
		arrays:   true
		required: bool
	}
	let Args = _args

	// `*` represents a wildcard type.
	//
	// For example, the `sinks.http.headers.*` option allows for arbitrary
	// key/value pairs.
	{"array": #TypeArray & {_args: required: Args.required}} |
	#TypePrimitive
}

#TypePrimitive: {
	_args: {
		arrays:   true
		required: bool
	}
	let Args = _args

	// `*` represents a wildcard type.
	//
	// For example, the `sinks.http.headers.*` option allows for arbitrary
	// key/value pairs.
	{"*": {}} |
	{"bool": #TypeBool & {_args: required: Args.required}} |
	{"float": #TypeFloat & {_args: required: Args.required}} |
	{"object": #TypeObject & {_args: required: Args.required}} |
	{"string": #TypeString & {_args: required: Args.required}} |
	{"timestamp": #TypeTimestamp & {_args: required: Args.required}} |
	{"uint": #TypeUint & {_args: required: Args.required}}
}

#TypeArray: {
	_args: required: bool
	_type: items: type: string
	let Args = _args
	let Type = _type

	if !Args.required {
		// `default` sets the default value.
		default: [...] | *null
	}

	examples?: [...[...Type.items.type]]

	// Set `required` to `true` to force disable defaults. Defaults should
	// be specified on the array level and not the type level.
	items: type: #TypePrimitive & {_args: required: true}
}

#TypeBool: {
	_args: required: bool
	let Args = _args

	if !Args.required {
		// `default` sets the default value.
		default: bool | *null
	}
}

#TypeFloat: {
	_args: required: bool
	let Args = _args

	if !Args.required {
		// `default` sets the default value.
		default: float | *null
	}

	// `examples` clarify values through examples. This should be used
	// when examples cannot be derived from the `default` or `enum`
	// options.
	examples?: [float, ...float]
}

#TypeObject: {
	// `examples` clarify values through examples. This should be used
	// when examples cannot be derived from the `default` or `enum`
	// options.
	examples: [#Object] | *[]

	// `options` represent the child options for this option.
	options: #Schema
}

#TypeString: {
	_args: required: bool
	let Args = _args

	if !Args.required {
		// `default` sets the default value.
		default: string | *null
	}

	// `enum` restricts the value to a set of values.
	//
	//      enum: {
	//        json: "Encodes the data via application/json"
	//        text: "Encodes the data via text/plain"
	//      }
	enum?: #Enum

	examples?: [...string]

	if Args.required && enum != _|_ {
		// `examples` demonstrates example values. This should be used when
		// examples cannot be derived from the `default` or `enum` options.
		examples: [string, ...string] | *[
				for k, v in enum {
				k
			},
		]
	}

	syntax: *"literal" | "file_system_path" | "field_path" | "template" | "regex" | "remap_boolean_expression" | "remap_program" | "strftime"
}

#TypeTimestamp: {
	_args: required: bool
	let Args = _args

	if !Args.required {
		// `default` sets the default value.
		default: #Timestamp | *null
	}

	// `examples` clarify values through examples. This should be used
	// when examples cannot be derived from the `default` or `enum`
	// options.
	examples: [_values.current_timestamp]
}

#TypeUint: {
	_args: required: bool
	let Args = _args

	if !Args.required {
		// `default` sets the default value.
		default: uint | *null
	}

	// `examples` clarify values through examples. This should be used
	// when examples cannot be derived from the `default` or `enum`
	// options.
	examples?: [uint, ...uint]

	// `unit` clarifies the value's unit. While this should be included
	// as the suffix in the name, this helps to explicitly clarify that.
	unit: #Unit | null
}

#Unit: "bytes" | "events" | "milliseconds" | "requests" | "seconds" | "lines"

administration: _
components:     _
configuration:  _
data_model:     _
glossary:       _
process:        _
releases:       _
remap:          _
