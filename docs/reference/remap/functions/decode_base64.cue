package metadata

remap: functions: decode_base64: {
	category:    "Codec"
	description: """
		Decodes the `value` (a [Base64](\(urls.base64)) string) into it's original string.
		"""

	arguments: [
		{
			name:        "value"
			description: "The [Base64](\(urls.base64)) data to decode."
			required:    true
			type: ["string"]
		},
		{
			name:        "padding"
			description: "Whether the Base64 output is [padded](\(urls.base64_padding))."
			required:    false
			type: ["boolean"]
			default: true
		},
		{
			name:        "charset"
			description: "The character set to use when encoding the data."
			required:    false
			type: ["string"]
			default: "standard"
			enum: {
				standard: "[Standard](\(urls.base64_standard)) Base64 format."
				url_safe: "Modified Base64 for [URL variants](\(urls.base64_url_safe))."
			}
		},
	]
	internal_failure_reasons: [
		"`value` isn't a valid encoded Base64 string.",
	]
	return: types: ["string"]

	examples: [
		{
			title: "Decode Base64 data (default)"
			source: """
				decode_base64("eW91IGhhdmUgc3VjY2Vzc2Z1bGx5IGRlY29kZWQgbWU=")
				"""
			return: "you have successfully decoded me"
		},
		{
			title: "Decode Base64 data (without padding)"
			source: """
				decode_base64("Z29vZCB0aW1lcyBuZXZlciBzZWVtZWQgc28gZ29vZA", padding: false)
				"""
			return: "good times never seemed so good"
		},
		{
			title: "Decode Base64 data (URL safe)"
			source: """
				decode_base64("eW91IGNhbid0IG1ha2UgeW91ciBoZWFydCBmZWVsIHNvbWV0aGluZyBpdCB3b24ndA==", charset: "url_safe")
				"""
			return: "you can't make your heart feel something it won't"
		},
	]
}
