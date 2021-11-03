package metadata

components: sinks: prometheus_exporter: {
	_port: 9598

	title: "Prometheus Exporter"
	alias: "prometheus"

	classes: {
		commonly_used: true
		delivery:      "best_effort"
		development:   "stable"
		egress_method: "expose"
		service_providers: []
		stateful: true
	}

	features: {
		buffer: enabled:      false
		healthcheck: enabled: false
		exposes: {
			tls: {
				enabled:                true
				can_enable:             true
				can_verify_certificate: true
				enabled_default:        false
			}

			for: {
				service: services.prometheus

				interface: {
					socket: {
						api: {
							title: "Prometheus text exposition format"
							url:   urls.prometheus_text_based_exposition_format
						}
						direction: "incoming"
						port:      _port
						protocols: ["http"]
						ssl: "disabled"
					}
				}
			}
		}
	}

	support: {
		targets: {
			"aarch64-unknown-linux-gnu":      true
			"aarch64-unknown-linux-musl":     true
			"armv7-unknown-linux-gnueabihf":  true
			"armv7-unknown-linux-musleabihf": true
			"x86_64-apple-darwin":            true
			"x86_64-pc-windows-msv":          true
			"x86_64-unknown-linux-gnu":       true
			"x86_64-unknown-linux-musl":      true
		}
		requirements: []
		warnings: [
			"""
				High cardinality metric names and labels are discouraged by Prometheus as they can provide performance
				and reliability problems. You should consider alternative strategies to reduce the cardinality. Vector
				offers a [`tag_cardinality_limit` transform](\(urls.vector_transforms)/tag_cardinality_limit) as a way
				to protect against this.
				""",
		]
		notices: []
	}

	configuration: {
		address: {
			description: "The address to expose for scraping."
			required:    true
			warnings: []
			type: string: {
				examples: ["0.0.0.0:\(_port)"]
			}
		}
		buckets: {
			common:      false
			description: """
				Default buckets to use for aggregating [distribution](\(urls.vector_data_model)/metric#distribution)
				metrics into histograms.
				"""
			required:    false
			type: array: {
				default: [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
				items: type: float: examples: [0.005, 0.01]
			}
		}
		flush_period_secs: {
			common:      false
			description: "Time interval between [set](\(urls.vector_data_model)/metric#set) values are reset."
			required:    false
			type: uint: {
				default: 60
				unit:    "seconds"
			}
		}
		default_namespace: {
			common:      true
			description: """
				Used as a namespace for metrics that don't have it. Typically
				namespaces are set during ingestion (sources), but it is
				optional and when missing, we'll use this value. It should
				follow Prometheus [naming conventions](\(urls.prometheus_metric_naming)).
				"""
			required:    false
			type: string: {
				default: null
				examples: ["service"]
			}
		}
		quantiles: {
			common:      false
			description: """
				Quantiles to use for aggregating [distribution](\(urls.vector_data_model)/metric#distribution) metrics
				into a summary.
				"""
			required:    false
			type: array: {
				default: [0.5, 0.75, 0.9, 0.95, 0.99]
				items: type: float: examples: [0.5, 0.75, 0.9, 0.95, 0.99]
			}
		}
	}

	input: {
		logs: false
		metrics: {
			counter:      true
			distribution: true
			gauge:        true
			histogram:    true
			set:          false
			summary:      true
		}
	}

	examples: [
		{
			_host:      _values.local_host
			_name:      "logins"
			_namespace: "service"
			_value:     1.5
			title:      "Counter"
			configuration: {
				default_namespace: _namespace
			}
			input: metric: {
				kind: "incremental"
				name: _name
				counter: {
					value: _value
				}
				tags: {
					host: _host
				}
			}
			output: """
				# HELP \(_namespace)_\(_name) \(_name)
				# TYPE \(_namespace)_\(_name) counter
				\(_namespace)_\(_name){host="\(_host)"} \(_value)
				"""
		},
		{
			_host:      _values.local_host
			_name:      "memory_rss"
			_namespace: "app"
			_value:     1.5
			title:      "Gauge"
			configuration: {}
			input: metric: {
				kind:      "absolute"
				name:      _name
				namespace: _namespace
				gauge: {
					value: _value
				}
				tags: {
					host: _host
				}
			}
			output: """
				# HELP \(_namespace)_\(_name) \(_name)
				# TYPE \(_namespace)_\(_name) gauge
				\(_namespace)_\(_name){host="\(_host)"} \(_value)
				"""
		},
		{
			_host: _values.local_host
			_name: "response_time_s"
			title: "Histogram"
			configuration: {
			}
			input: metric: {
				kind: "absolute"
				name: _name
				histogram: {
					buckets: [
						{upper_limit: 0.005, count: 0},
						{upper_limit: 0.01, count:  1},
						{upper_limit: 0.025, count: 0},
						{upper_limit: 0.05, count:  1},
						{upper_limit: 0.1, count:   0},
						{upper_limit: 0.25, count:  0},
						{upper_limit: 0.5, count:   0},
						{upper_limit: 1.0, count:   0},
						{upper_limit: 2.5, count:   0},
						{upper_limit: 5.0, count:   0},
						{upper_limit: 10.0, count:  0},
					]
					count: 2
					sum:   0.789
				}
			}
			output: """
				# HELP \(_name) \(_name)
				# TYPE \(_name) histogram
				\(_name)_bucket{le="0.005"} 0
				\(_name)_bucket{le="0.01"} 1
				\(_name)_bucket{le="0.025"} 0
				\(_name)_bucket{le="0.05"} 1
				\(_name)_bucket{le="0.1"} 0
				\(_name)_bucket{le="0.25"} 0
				\(_name)_bucket{le="0.5"} 0
				\(_name)_bucket{le="1.0"} 0
				\(_name)_bucket{le="2.5"} 0
				\(_name)_bucket{le="5.0"} 0
				\(_name)_bucket{le="10.0"} 0
				\(_name)_bucket{le="+Inf"} 0
				\(_name)_sum 0.789
				\(_name)_count 2
				"""
		},
		{
			_host: _values.local_host
			_name: "request_retries"
			title: "Distribution to histogram"
			notes: "Histogram will be computed out of values and then passed to prometheus."
			configuration: {
				buckets: [0.0, 1.0, 3.0]
			}
			input: metric: {
				name: _name
				kind: "incremental"
				distribution: {
					samples: [
						{value: 0.0, rate: 4},
						{value: 1.0, rate: 2},
						{value: 4.0, rate: 1},
					]
					statistic: "histogram"
				}
				tags: {
					host: _host
				}
			}
			output: """
				# HELP \(_name) \(_name)
				# TYPE \(_name) histogram
				\(_name)_bucket{host="\(_host)",le="0"} 4
				\(_name)_bucket{host="\(_host)",le="1"} 6
				\(_name)_bucket{host="\(_host)",le="3"} 6
				\(_name)_bucket{host="\(_host)",le="+Inf"} 7
				\(_name)_sum{host="\(_host)"} 6
				\(_name)_count{host="\(_host)"} 7
				"""
		},
		{
			_host: _values.local_host
			_name: "request_retries"
			title: "Distribution to summary"
			notes: "Summary will be computed out of values and then passed to prometheus."
			configuration: {
				quantiles: [0.5, 0.75, 0.95]
			}
			input: metric: {
				name: _name
				kind: "incremental"
				distribution: {
					samples: [
						{value: 0.0, rate: 3},
						{value: 1.0, rate: 2},
						{value: 4.0, rate: 1},
					]
					statistic: "summary"
				}
			}
			output: """
				# HELP \(_name) \(_name)
				# TYPE \(_name) summary
				\(_name){quantile="0.5"} 0
				\(_name){quantile="0.75"} 1
				\(_name){quantile="0.95"} 4
				\(_name)_sum 6
				\(_name)_count 6
				\(_name)_min 0
				\(_name)_max 4
				\(_name)_avg 1
				"""
		},
		{
			_host: _values.local_host
			_name: "requests"
			title: "Summary"
			configuration: {}
			input: metric: {
				name: _name
				kind: "absolute"
				summary: {
					quantiles: [
						{upper_limit: 0.01, value: 1.5},
						{upper_limit: 0.5, value:  2.0},
						{upper_limit: 0.99, value: 3.0},
					]
					count: 6
					sum:   12.0
				}
				tags: {
					host: _host
				}
			}
			output: """
				# HELP \(_name) \(_name)
				# TYPE \(_name) summary
				\(_name){host="\(_host)",quantile="0.01"} 1.5
				\(_name){host="\(_host)",quantile="0.5"} 2
				\(_name){host="\(_host)",quantile="0.99"} 3
				\(_name)_sum{host="\(_host)"} 12
				\(_name)_count{host="\(_host)"} 6
				"""
		},
	]

	how_it_works: {
		histogram_buckets: {
			title: "Histogram Buckets"
			body:  """
				Choosing the appropriate buckets for Prometheus histograms is a complicated point of
				discussion. The [Histograms and Summaries Prometheus guide](\(urls.prometheus_histograms_guide))
				provides a good overview of histograms, buckets, summaries, and how you should think
				about configuring them. The buckets you choose should align with your known range
				and distribution of values as well as how you plan to report on them. The
				aforementioned guide provides examples on how you should align them.
				"""
			sub_sections: [
				{
					title: "Default Buckets"
					body: """
						The `buckets` option defines the global default buckets for histograms.
						These defaults are tailored to broadly measure the response time (in seconds)
						of a network service. Most likely, however, you will be required to define
						buckets customized to your use case.
						"""
				},
			]
		}

		memory_usage: {
			title: "Memory Usage"
			body: """
				Like other Prometheus instances, the `prometheus` sink aggregates
				metrics in memory which keeps the memory footprint to a minimum if Prometheus
				fails to scrape the Vector instance over an extended period of time. The
				downside is that data will be lost if Vector is restarted. This is by design of
				Prometheus' pull model approach, but is worth noting if restart Vector
				frequently.
				"""
		}
	}
}
