package metadata

components: sources: http_scrape: {
	title: "HTTP Scrape"
	alias: "http_scrape"

	classes: {
		commonly_used: false
		delivery:      "at_least_once"
		deployment_roles: ["daemon", "sidecar"]
		development:   "beta"
		egress_method: "batch"
		stateful:      false
	}

	features: {
		acknowledgements: false
		codecs: {
			enabled:         true
			default_framing: "`bytes`"
		}
		collect: {
			checkpoint: enabled: false
			from: {
				service: services.http_scrape

				interface: socket: {
					direction: "outgoing"
					protocols: ["http"]
					ssl: "optional"
				}
			}
			proxy: enabled: true
			tls: {
				enabled:                true
				can_verify_certificate: true
				can_verify_hostname:    true
				enabled_default:        false
			}
		}
		multiline: enabled: false
	}

	support: {
		requirements: []
		warnings: []
		notices: []
	}

	installation: {
		platform_name: null
	}

	configuration: {
		auth: configuration._http_auth & {_args: {
			password_example: "${PASSWORD}"
			username_example: "${USERNAME}"
		}}
		endpoint: {
			description: "Endpoint to scrape observability data from."
			required:    true
			warnings: ["You must explicitly add the path to your endpoint."]
			type: string: {
				examples: ["http://127.0.0.1:9898/logs"]
			}
		}
		headers: {
			common:      false
			description: "A list of HTTP headers to include in request."
			required:    false
			type: object: {
				examples: [{"Your-Custom-Header": "it's-value"}]
			}
		}
		query: {
			common: false
			description: """
				Custom parameters for the scrape request query string.
				One or more values for the same parameter key can be provided.
				The parameters provided in this option are appended to the `endpoint` option.
				"""
			required: false
			type: object: {
				examples: [{"match[]": [#"{job="somejob"}"#, #"{__name__=~"job:.*"}"#]}]
				options: {
					"*": {
						common:      false
						description: "Any query key"
						required:    false
						type: array: {
							default: null
							examples: [[
								#"{job="somejob"}"#,
								#"{__name__=~"job:.*"}"#,
							]]
							items: type: string: {
								examples: [
									#"{job="somejob"}"#,
									#"{__name__=~"job:.*"}"#,
								]
								syntax: "literal"
							}
						}
					}
				}
			}
		}
		scrape_interval_secs: {
			common:      true
			description: "The interval between scrapes, in seconds."
			required:    false
			type: uint: {
				default: 15
				unit:    "seconds"
			}
		}
	}

	output: metrics: {
		counter:      output._passthrough_counter
		distribution: output._passthrough_distribution
		gauge:        output._passthrough_gauge
		histogram:    output._passthrough_histogram
		set:          output._passthrough_set
	}

	telemetry: metrics: {
		events_in_total:                      components.sources.internal_metrics.output.metrics.events_in_total
		http_error_response_total:            components.sources.internal_metrics.output.metrics.http_error_response_total
		http_request_errors_total:            components.sources.internal_metrics.output.metrics.http_request_errors_total
		parse_errors_total:                   components.sources.internal_metrics.output.metrics.parse_errors_total
		processed_bytes_total:                components.sources.internal_metrics.output.metrics.processed_bytes_total
		processed_events_total:               components.sources.internal_metrics.output.metrics.processed_events_total
		component_discarded_events_total:     components.sources.internal_metrics.output.metrics.component_discarded_events_total
		component_errors_total:               components.sources.internal_metrics.output.metrics.component_errors_total
		component_received_bytes_total:       components.sources.internal_metrics.output.metrics.component_received_bytes_total
		component_received_event_bytes_total: components.sources.internal_metrics.output.metrics.component_received_event_bytes_total
		component_received_events_total:      components.sources.internal_metrics.output.metrics.component_received_events_total
		requests_completed_total:             components.sources.internal_metrics.output.metrics.requests_completed_total
		request_duration_seconds:             components.sources.internal_metrics.output.metrics.request_duration_seconds
	}
}
