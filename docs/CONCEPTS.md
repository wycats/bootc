# Your Desktop as Code: A Guide to Atomic Linux with bkt

_A conceptual guide for developers new to atomic/immutable Linux_

---

## Part 1: The Problem

You've been using Linux for years. Your machine works... mostly. But you can't remember:

- Why you installed that PPA three years ago
- What that random binary in `/usr/local/bin` does
- Whether you can safely remove `libfoo-dev`

Your machine is a **snowflake** — unique, unreproducible, and slowly accumulating cruft.

---

## Part 2: The Atomic Linux Promise

Atomic Linux (Fedora Silverblue, Bazzite, etc.) fixes this for the base system by making `/usr` **immutable**. Your OS is a container image that gets replaced atomically on updates.

```bash
$ bootc status
Current: ghcr.io/ublue-os/bazzite-gnome:stable
```

**But there's a gap.** Atomic Linux drew the line at `/usr`. Everything you actually interact with daily — Flatpaks, GNOME extensions, settings, dev tools — is still unmanaged mutable state.

---

## Part 3: What if we could capture that too?

Not by changing how you work. You still:

- Install Flatpaks with GNOME Software
- Enable extensions with Extension Manager
- Install dev tools with `cargo install`
- Tweak settings in GNOME Settings

**bkt can capture what you've done and write it down.**

```bash
# You installed some stuff the normal way...
# Now capture it:
$ bkt capture
→ Found 3 new Flatpaks
→ Found 2 new extensions
→ Found 12 changed settings
→ Updated manifests with your changes
```

Your manifest now describes what you have. It's a **no-op against your current machine** — but it can **bootstrap a fresh one**.

---

## Part 4: The Try-Before-You-Buy Zone

Not everything you install deserves to be permanent:

- That CLI tool you tried once for debugging
- The extension that turned out to be buggy
- The Flatpak you're evaluating

**Your desktop is a draft. The manifest is the published version.**

```bash
# Try something
$ flatpak install flathub com.example.MaybeCool

# Live with it for a while...
# Decide you like it...

# Now make it official
$ bkt capture
→ Added com.example.MaybeCool to flatpak-apps.json
```

Only capture when you're confident. The manifest stays clean.

---

## Part 5: The Dev Sandbox

Dev tools have the same "try before you buy" nature, but more so:

- You experiment with new compilers
- You install random crates to test things
- You switch Node versions for different projects

The **dev sandbox** is a container that:

- Shares your `$HOME` (your code is right there)
- Has a mutable `/usr` (you can install anything)
- Is invisible (commands just work)

```bash
# This just works — you don't think about containers
$ cargo install ripgrep
$ rg "TODO" ~/Code
```

---

## Part 6: Why a Sandbox?

The base image is your **published release** — stable, tested, shared.

The sandbox is your **working draft** — where you figure out what you actually need.

|              | Base Image            | Sandbox / User-space |
| ------------ | --------------------- | -------------------- |
| Update speed | Minutes + reboot      | Seconds              |
| Commitment   | Deliberate            | Exploratory          |
| Sharing      | Same for all machines | Personal experiments |

You iterate in the sandbox, then capture what works into the manifest.

---

## Part 7: Capture Semantics

The hard work bkt does is defining **what "capture" means** for each domain:

| Domain     | What gets captured               |
| ---------- | -------------------------------- |
| Flatpak    | Installed app IDs and remotes    |
| Extensions | Enabled extension UUIDs          |
| GSettings  | Settings you've explicitly chosen to track |
| Dev tools  | Binaries in `~/.cargo/bin`, etc. |

Each subsystem has its own logic for separating signal from noise.

---

## Part 8: The Payoff

**New machine?**

On first login, the system bootstraps itself from the manifests baked
into the image — installing Flatpaks, enabling extensions, applying
settings. After that, `bkt apply` keeps things in sync:

```bash
$ bkt apply
→ Syncing 24 Flatpaks...
→ Enabling 8 extensions...
→ Applying 47 settings...
→ Setting up dev environment...
✓ Done. Your desktop is ready.
```

Same apps. Same extensions. Same settings. Same dev tools.

**Your desktop is reproducible.**

---

## Part 9: The Full Picture

```
┌─────────────────────────────────────────────────────────┐
│                  Your Daily Workflow                     │
│                                                          │
│   GNOME Software, Extension Manager, cargo install,      │
│   flatpak install, gsettings, normal Linux stuff         │
└─────────────────────────────────────────────────────────┘
                         │
                         ▼ bkt capture
┌─────────────────────────────────────────────────────────┐
│                    Your Repository                       │
│                                                          │
│   manifests/*.json — what you've decided to keep         │
└─────────────────────────────────────────────────────────┘
                         │
                         ▼ bkt apply (on new machine)
┌─────────────────────────────────────────────────────────┐
│                    Reproduced Desktop                    │
│                                                          │
│   Same apps, extensions, settings, dev tools             │
└─────────────────────────────────────────────────────────┘
```

**Use normal tools → capture when ready → reproduce anywhere.**

---

## Part 10: The Base Image (For Power Users)

Some things can't be captured — they need to be baked into the OS image:

- System packages (gcc, fonts, codecs)
- Kernel arguments
- Systemd units

For these, bkt manages your **Containerfile**:

```bash
$ bkt system add htop
→ Added htop to system-packages.json
→ Created PR #48
→ After merge: CI builds new image
→ After reboot: htop is there
```

This is the slow path — but it's also the stable path.

---

## Summary

| Concept         | What it means                                    |
| --------------- | ------------------------------------------------ |
| **Atomic base** | Your OS is an immutable image                    |
| **User-space**  | Apps, extensions, settings — mutable, capturable |
| **Dev sandbox** | Where you experiment with dev tools              |
| **Capture**     | Snapshot current state into manifests            |
| **Apply**       | Make a machine match the manifests               |
| **Manifest**    | The source of truth for reproduction             |

**bkt doesn't change how you use Linux. It makes what you do reproducible.**

---

## The One-Liner

> **Use your desktop normally. Capture what works. Reproduce it anywhere.**
