package metadata

components: sinks: datadog_logs: {
  title: "Datadog Logs"
  short_description: "Streams log events to [Datadog's][urls.datadog] logs via the [TCP endpoint][urls.datadog_logs_endpoints]."
  long_description: "[Datadog][urls.datadog] is a monitoring service for cloud-scale applications, providing monitoring of servers, databases, tools, and services, through a SaaS-based data analytics platform."

  classes: {
    commonly_used: false
    function: "transmit"
    service_providers: ["Datadog"]
  }

  features: {
    batch: enabled: false
    buffer: enabled: true
    compression: enabled: false
    encoding: {
      enabled: true
      default: null
      json: null
      ndjson: null
      text: null
    }
    healthcheck: enabled: true
    request: enabled: false
    tls: {
      enabled: true
      can_enable: true
      can_verify_certificate: true
      can_verify_hostname: true
      enabled_default: true
    }
  }

  statuses: {
    delivery: "at_least_once"
    development: "beta"
  }

  support: {
    input_types: ["log"]

    platforms: {
      "aarch64-unknown-linux-gnu": true
      "aarch64-unknown-linux-musl": true
      "x86_64-apple-darwin": true
      "x86_64-pc-windows-msv": true
      "x86_64-unknown-linux-gnu": true
      "x86_64-unknown-linux-musl": true
    }

    requirements: []
    warnings: []
  }

  configuration: {
    api_key: {
      description: "Datadog [API key](https://docs.datadoghq.com/api/?lang=bash#authentication)"
      required: true
      warnings: []
      type: string: {
        examples: ["${DATADOG_API_KEY_ENV_VAR}","ef8d5de700e7989468166c40fc8a0ccd"]
      }
    }
  }
}

