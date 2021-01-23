package metadata

installation: _interfaces: dpkg: {
	title:       "DPKG"
	description: """
		[Dpkg](\(urls.dpkg)) is the software that powers the package management
		system in the Debian operating system and its derivatives. Dpkg is used
		to install and manage software via `.deb` packages.
		"""

	archs: ["x86_64", "ARM64", "ARMv7"]
	package_manager_name: installation.package_managers.dpkg.name

	paths: {
		bin:         "/usr/bin/vector"
		bin_in_path: true
		config:      "/etc/vector/vector.{config_format}"
	}

	roles: [Name=string]: {
		commands: roles._systemd_commands & {
			_config_path: paths.config
			install: #"""
				curl --proto '=https' --tlsv1.2 -O https://packages.timber.io/vector/{version}/vector-{version}-{arch}.deb && \
					sudo dpkg -i vector-{version}-{arch}.deb
				"""#
			uninstall: "sudo dpkg -r vector"
			upgrade:   null
		}
		tutorials: {
			_commands: _
			installation: [
				{
					title:   "Install Vector"
					command: commands.install
				},
				{
					title:   "Configure Vector"
					command: commands.configure
				},
				{
					title:   "Restart Vector"
					command: commands.restart
				},
			]
		}
		variables: {
			arch: ["amd64", "arm64", "armhf"]
			version: true
		}
	}

	roles: {
		agent:      roles._journald_agent
		aggregator: roles._vector_aggregator
	}
}
