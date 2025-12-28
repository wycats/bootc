# Nushell environment for new users.
#
# Note: your current toolbox setup uses a custom toolbox image tag that already
# bakes a PATH like:
#   /home/wycats/.nix-profile/bin:/home/wycats/.cargo/bin:/home/wycats/.local/bin:...
#
# If you prefer PATH management in shell init instead of the toolbox image,
# you can uncomment the block below.

# let extra_paths = [
#   ($env.HOME | path join ".nix-profile" "bin")
#   ($env.HOME | path join ".cargo" "bin")
#   ($env.HOME | path join ".local" "bin")
#   ($env.HOME | path join "bin")
# ]
#
# let existing_extra = ($extra_paths | where {|p| ($p | path exists) })
#
# $env.PATH = ($env.PATH | prepend $existing_extra)

