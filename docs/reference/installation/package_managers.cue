package metadata

installation: {
	#PackageManager: {
		description: string
		name:        string
		title:       string
	}

	#PackageManagers: [Name=string]: #PackageManager & {
		name: Name
	}

	package_managers: #PackageManagers
}
