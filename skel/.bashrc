# ~/.bashrc

# ==============================================================================
# PATH is set by ~/.config/environment.d/10-distrobox-exports.conf
# Do NOT modify PATH here - use environment.d for reliable, shell-agnostic config.
# ==============================================================================

# Source global definitions
if [ -f /etc/bashrc ]; then
    . /etc/bashrc
fi

# User specific aliases
if [ -d ~/.bashrc.d ]; then
    for rc in ~/.bashrc.d/*; do
        if [ -f "$rc" ]; then . "$rc"; fi
    done
fi
unset rc

# ==============================================================================
# INTERACTIVE SHELL: Hand off to nushell if available
# ==============================================================================

if [[ $- == *i* ]] && [ -x "$(command -v nu)" ] && [ -z "$NU_VERSION" ] && [[ "$TERM_PROGRAM" != "vscode" ]]; then
    # Starship Init
    if command -v starship &> /dev/null; then
        mkdir -p ~/.cache/starship
        if [ ! -f ~/.cache/starship/init.nu ]; then
            starship init nu > ~/.cache/starship/init.nu
        fi
    fi

    # MOTD
    if [ -f /run/host/etc/motd ]; then
        cat /run/host/etc/motd
    fi

    # Switch to nushell
    exec nu
fi

# Bazzite CLI bling
test -f /usr/share/bazzite-cli/bling.sh && source /usr/share/bazzite-cli/bling.sh

# GPG pinentry support
export GPG_TTY=$(tty)
