# Secrets Management

This image uses 1Password CLI for secrets. **Never commit secrets to this repository.**

## Prerequisites

1. Install 1Password desktop app (via Flatpak: `org.1password.app`)
2. Enable CLI integration in 1Password settings:
   - Open 1Password → Settings → Developer
   - Enable "Integrate with 1Password CLI"

## Usage

### Read a secret

```bash
op read "op://Personal/GitHub Token/credential"
```

### Inject into environment

```bash
export GITHUB_TOKEN=$(op read "op://Personal/GitHub Token/credential")
```

### Use in scripts

```bash
# Authenticate GitHub CLI
gh auth login --with-token <<< $(op read "op://Personal/GitHub Token/credential")
```

### Use in ujust recipes

```just
some-recipe:
    #!/usr/bin/env bash
    export API_KEY=$(op read "op://Personal/API Key/credential")
    curl -H "Authorization: Bearer $API_KEY" https://api.example.com
```

## Secret Reference Format

1Password uses the format: `op://<vault>/<item>/<field>`

- **vault**: The 1Password vault name (e.g., "Personal", "Work")
- **item**: The item title in 1Password
- **field**: The field name (commonly "credential", "password", or custom field names)

## Troubleshooting

### "not signed in" error

Run `op signin` or unlock 1Password in the desktop app. With CLI integration enabled, the CLI uses the desktop app's session.

### "item not found" error

Check the vault name and item title match exactly. Use `op item list` to see available items.

## Security Notes

- Secrets are never written to disk; they're fetched on-demand
- The 1Password desktop app handles secure storage and biometric unlock
- CLI sessions are tied to the desktop app session
