package metadata

remap2: features: {
	#Feature: {
		name:        string
		title:       string
		description: string

		features: [Name=string]: #Feature & {
			name: Name
		}
	}

	{[Name=string]: #Feature & {
		name: Name
	}}
}
