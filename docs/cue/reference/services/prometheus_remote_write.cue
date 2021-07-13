package metadata

services: prometheus_remote_write: {
	name:     "Prometheus Remote Write Integrations"
	thing:    "a metrics database or service"
	url:      urls.prometheus_remote_integrations
	versions: null

	description: """
		Databases and services that are capable of receiving data via the Prometheus [`remote_write`](\(urls.prometheus_remote_write_protocol)).
	"""
}
