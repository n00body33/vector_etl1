package metadata

remap: functions: replace: {
	category: "String"
	description: """
		Replaces all matching instances of `pattern` in the `value`.

		The `pattern` argument accepts regular expression capture groups. Note that `$foo` is interpreted in a configuration file, instead use `$$foo`.
		"""

	arguments: [
		{
			name:        "value"
			description: "The original string."
			required:    true
			type: ["string"]
		},
		{
			name:        "pattern"
			description: "Replace all matches of this pattern. Can be a static string or a regular expression."
			required:    true
			type: ["regex", "string"]
		},
		{
			name:        "with"
			description: "The string that the matches are replaced with."
			required:    true
			type: ["string"]
		},
		{
			name:        "count"
			description: "The maximum number of replacements to perform. -1 means replace all matches."
			required:    false
			default:     -1
			type: ["integer"]

		},
	]
	internal_failure_reasons: []
	return: types: ["string"]

	examples: [
		{
			title: "Replace literal text"
			source: #"""
				replace("Apples and Bananas", "and", "not")
				"""#
			return: "Apples not Bananas"
		},
		{
			title: "Replace via regular expression"
			source: #"""
				replace("Apples and Bananas", r'(?i)bananas', "Pineapples")
				"""#
			return: "Apples and Pineapples"
		},
		{
			title: "Replace first instance"
			source: #"""
				replace("Bananas and Bananas", "Bananas", "Pineapples", count: 1)
				"""#
			return: "Pineapples and Bananas"
		},
		{
			title: "Replace with capture groups when not set in the configuration file"
			source: #"""
				replace("foo123bar", r'foo(?P<num>\d+)bar', "$num")
				"""#
			return: "123"
		},
		{
			title: "Replace with capture groups only in configuration file (requires escaping `$`)"
			source: #"""
				replace("foo123bar", r'foo(?P<num>\d+)bar', "$$num")
				"""#
			return: "123"
		},
	]
}
