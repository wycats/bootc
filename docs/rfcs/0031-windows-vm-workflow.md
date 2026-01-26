# RFC-0031: Windows VM Workflow

## Status: Draft

## Summary

Provide an opinionated, automated answer to "I need to run a Windows app" via `bkt vm`. This is not a general-purpose VM manager—it's a single blessed path optimized for the common case of needing Windows for a specific application.

## Motivation

Running Windows apps on Linux has multiple approaches with different tradeoffs:

| Approach          | Complexity                | Performance | Compatibility                |
| ----------------- | ------------------------- | ----------- | ---------------------------- |
| Wine/Proton       | Low                       | Native      | Hit or miss                  |
| Bottles           | Medium                    | Native      | Better than raw Wine         |
| **VM (this RFC)** | High initial, low ongoing | Good enough | 100%                         |
| Dual boot         | High                      | Native      | 100% but context-switch pain |

For apps that don't work in Wine (e.g., enterprise software, specific hardware drivers, some creative tools), a VM is the right answer. But the setup is tedious:

1. Download Windows ISO
2. Create VM with correct settings (TPM, UEFI, Secure Boot for Win11)
3. Install Windows
4. Install VirtIO drivers for performance
5. Install SPICE guest tools for clipboard/resolution
6. Enable RDP for smoother daily use
7. Configure firewall rules

This RFC proposes automating the repeatable parts while guiding the user through the interactive parts.

## Design

### Command Structure

```bash
# First-time setup (interactive, ~30 min)
bkt vm create windows

# Daily use
bkt vm start windows      # Start VM in background
bkt vm connect windows    # Connect via RDP (or SPICE fallback)
bkt vm stop windows       # Graceful shutdown

# Maintenance
bkt vm status             # Show all VMs and their state
bkt vm destroy windows    # Delete VM and disk
```

### The `bkt vm create windows` Workflow

1. **Preflight checks**
   - Verify KVM support (`/dev/kvm` exists)
   - Check available disk space (80GB+ recommended)
   - Verify libvirt is running
   - Check for existing VMs with same name

2. **Guided setup** (interactive)

   ```
   Windows 11 VM Setup
   ═══════════════════

   This will create a Windows 11 VM optimized for running Windows apps.

   ▸ VM Name: windows (default) or custom
   ▸ Disk size: 80GB (default, grows as needed)
   ▸ RAM: 8GB (default, 50% of system RAM max)
   ▸ CPUs: 4 (default, 50% of cores)

   Required: Windows 11 ISO
   ▸ Path to ISO: [user provides path]

   We'll also download:
   ▸ VirtIO drivers (for disk/network performance)
   ▸ SPICE guest tools (for clipboard/resolution)
   ```

3. **Automated VM creation**
   - Generate libvirt XML with optimal settings
   - TPM 2.0 emulation, UEFI, Secure Boot
   - QXL video with 128MB VRAM (high-res support)
   - VirtIO disk and network
   - SPICE graphics with clipboard channel

4. **Guided Windows installation**

   ```
   VM created and started. Complete Windows installation:

   1. A window will open with the Windows installer
   2. Choose "I don't have a product key" (activate later)
   3. Select Windows 11 Pro
   4. Choose "Custom: Install Windows only"
   5. Click "Load driver" → browse to virtio-win CD → amd64\w11
   6. Select the VirtIO disk and continue installation

   [Press Enter when installation completes and you're at the desktop]
   ```

5. **Post-install automation**
   - Prompt user to install SPICE Guest Tools (from attached ISO)
   - Enable RDP via registry (with user consent)
   - Open firewall for RDP
   - Store VM metadata in manifest

### The `bkt vm connect` Command

```bash
bkt vm connect windows
```

1. Check if VM is running, start if not
2. Get VM IP address via `virsh domifaddr`
3. Attempt RDP connection via `xfreerdp` or `remmina`
4. Fall back to SPICE (`virt-viewer`) if RDP fails

### Manifest Storage

```json
// ~/.config/bootc/vms.json
{
  "vms": [
    {
      "name": "windows",
      "type": "windows11",
      "disk": "/var/lib/libvirt/images/windows.qcow2",
      "created": "2026-01-25",
      "rdp_enabled": true,
      "notes": "For Bambu Studio slicer"
    }
  ]
}
```

This is informational only—the source of truth is libvirt. The manifest tracks our metadata (why the VM exists, what apps it's for).

### What We DON'T Automate

1. **Windows installation itself** - Too many dialogs, user choices
2. **Windows activation** - User's responsibility
3. **Application installation** - Beyond scope
4. **GPU passthrough** - Requires 2 GPUs, hardware-specific

### Why Not Just Use GNOME Boxes?

GNOME Boxes is good for quick Linux VMs but lacks:

- TPM 2.0 for Windows 11
- VirtIO driver integration
- RDP setup automation
- Opinionated defaults for our use case

## Implementation Plan

### Phase 1: Core Commands

- [ ] `bkt vm create windows` - Guided VM creation
- [ ] `bkt vm start <name>` - Start VM
- [ ] `bkt vm stop <name>` - Stop VM
- [ ] `bkt vm status` - List VMs

### Phase 2: Connection

- [ ] `bkt vm connect <name>` - Smart connect (RDP preferred)
- [ ] Automatic IP detection
- [ ] RDP client integration (xfreerdp/remmina)

### Phase 3: Automation Helpers

- [ ] Download VirtIO ISO if missing
- [ ] Download SPICE tools if missing
- [ ] Post-install RDP enablement script

### Phase 4: Polish

- [ ] VM health checks
- [ ] Snapshot support for "known good" state
- [ ] Resource usage warnings

## Technical Details

### Optimal Windows 11 VM Settings

```xml
<domain type='kvm'>
  <memory unit='GiB'>8</memory>
  <vcpu>4</vcpu>
  <os firmware='efi'>
    <type arch='x86_64' machine='pc-q35-10.1'>hvm</type>
    <firmware>
      <feature enabled='yes' name='secure-boot'/>
    </firmware>
  </os>
  <features>
    <acpi/><apic/>
    <hyperv mode='passthrough'/>
    <smm state='on'/>
  </features>
  <cpu mode='host-passthrough'/>
  <devices>
    <!-- VirtIO disk for performance -->
    <disk type='file' device='disk'>
      <driver name='qemu' type='qcow2' discard='unmap'/>
      <target dev='vda' bus='virtio'/>
    </disk>
    <!-- VirtIO network -->
    <interface type='network'>
      <source network='default'/>
      <model type='virtio'/>
    </interface>
    <!-- QXL for high-res + SPICE -->
    <video>
      <model type='qxl' ram='131072' vram='131072' vgamem='65536'/>
    </video>
    <graphics type='spice' autoport='yes'/>
    <!-- TPM 2.0 for Windows 11 -->
    <tpm model='tpm-crb'>
      <backend type='emulator' version='2.0'/>
    </tpm>
  </devices>
</domain>
```

### RDP Enablement

**Option 1: Via autounattend.xml (preferred, runs during install)**

This enables RDP during the specialize pass, before BitLocker is enabled:

```xml
<settings pass="specialize">
  <component name="Microsoft-Windows-TerminalServices-LocalSessionManager"
    processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35"
    language="neutral" versionScope="nonSxS">
    <fDenyTSConnections>false</fDenyTSConnections>
  </component>
  <component name="Microsoft-Windows-TerminalServices-RDP-WinStationExtensions"
    processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35"
    language="neutral" versionScope="nonSxS">
    <UserAuthentication>1</UserAuthentication>
  </component>
  <component name="Networking-MPSSVC-Svc"
    processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35"
    language="neutral" versionScope="nonSxS">
    <FirewallGroups>
      <FirewallGroup wcm:action="add" wcm:keyValue="RemoteDesktop">
        <Active>true</Active>
        <Group>Remote Desktop</Group>
        <Profile>all</Profile>
      </FirewallGroup>
    </FirewallGroups>
  </component>
</settings>
```

**Option 2: Via FirstLogonCommands (for post-install automation)**

If the VM already exists, attach a script ISO:

```powershell
# enable-rdp.ps1
Set-ItemProperty -Path 'HKLM:\System\CurrentControlSet\Control\Terminal Server' -Name "fDenyTSConnections" -Value 0
Enable-NetFirewallRule -DisplayGroup "Remote Desktop"
Set-ItemProperty -Path 'HKLM:\System\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp' -Name "UserAuthentication" -Value 1
```

**Why virt-customize doesn't work**: BitLocker encrypts the Windows partition. Offline registry modification requires either:

- Disabling BitLocker (defeats the security purpose)
- Using autounattend.xml during initial install (no BitLocker yet)
- Running scripts from within Windows (what we do with script ISO)

### Connection Priority

1. **RDP** (port 3389) - Best for daily use, smoother than SPICE
2. **SPICE** - Fallback, needed for boot/troubleshooting
3. **VNC** - Not used (SPICE is superior)

## Alternatives Considered

### 1. Declarative VM definitions in manifests

Too complex for the "one Windows VM" use case. Could add later if needed.

### 2. Integrate with Quickemu

Quickemu has similar goals but is shell-based and has its own opinions. Better to have native Rust implementation aligned with bkt patterns.

### 3. Just document the manual steps

Current state. Works but tedious and error-prone.

## Open Questions

1. Should we support multiple Windows VMs? (Probably not initially)
2. Should we support other guest OSes? (Linux VMs are easy enough without automation)
3. How to handle Windows Updates gracefully?
4. Snapshot strategy for "golden image" before risky changes?

## References

- [libvirt domain XML format](https://libvirt.org/formatdomain.html)
- [VirtIO Windows drivers](https://github.com/virtio-win/virtio-win-pkg-scripts)
- [SPICE guest tools](https://www.spice-space.org/download.html)
- Current manual setup: This conversation (2026-01-25)
