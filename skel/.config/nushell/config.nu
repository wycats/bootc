# config.nu
#
# Installed by:
# version = "0.108.0"
#
# This file is used to override default Nushell settings, define
# (or import) custom commands, or run any other startup tasks.
# See https://www.nushell.sh/book/configuration.html
#
# Nushell sets "sensible defaults" for most configuration settings, 
# so your `config.nu` only needs to override these defaults if desired.
#
# You can open this file in your default editor using:
#     config nu
#
# You can also pretty-print and page through the documentation for configuration
# options using:
#     config nu --doc | nu-highlight | less -R
$env.config = {
    show_banner: true,
    hooks: {
        pre_prompt: [{ ||
            if (which direnv | is-empty) {
                return
            }
            direnv export json | from json | default {} | load-env
        }]
    }
}

$env.EDITOR = "code --wait"
$env.VISUAL = "code --wait"

# Load it
source ~/.cache/starship/init.nu

# Append Nix profile to PATH
$env.PATH = ($env.PATH | split row (char esep) | prepend $"($env.HOME)/.nix-profile/bin")

# Optional: Load Nix environment variables (if you need NIX_SSL_CERT_FILE etc)
# Since parsing bash scripts in Nu is hard, you might just want to hardcode the critical ones:
$env.NIX_SSL_CERT_FILE = "/etc/ssl/certs/ca-bundle.crt" # Standard Fedora path

if (which direnv | is-not-empty) {
    direnv export json | from json | default {} | load-env
}
# bootc: required for gpg pinentry in terminals
if $nu.is-interactive {
  if not (which tty | is-empty) {
    $env.GPG_TTY = (tty | str trim)
  }
}
