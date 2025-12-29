# Nushell config for new users.
# Replace with your real config:
#   cp -a ~/.config/nushell/config.nu ./skel/.config/nushell/config.nu


# bootc: required for gpg pinentry in terminals
if $nu.is-interactive {
	if not (which tty | is-empty) {
		$env.GPG_TTY = (tty | str trim)
	}
}

