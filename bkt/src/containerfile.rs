//! Containerfile auto-generation and section management.
//!
//! This module provides functionality to automatically update managed sections
//! of the Containerfile when manifests change. It preserves manual content
//! outside of marked sections.
//!
//! # Section Markers
//!
//! Managed sections are delimited by special comments:
//!
//! ```dockerfile
//! # === SECTION_NAME (managed by bkt) ===
//! # Auto-generated content here
//! # === END SECTION_NAME ===
//! ```
//!
//! # Supported Sections
//!
//! - `SYSTEM_PACKAGES`: RPM packages from system-packages.json
//! - `COPR_REPOS`: COPR repository enablement commands
//! - `HOST_SHIMS`: Host shim COPY and symlink commands

use crate::manifest::Shim;
use crate::manifest::system_config::SystemConfigManifest;
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use std::fs;
use std::path::{Path, PathBuf};

/// Marker prefix for managed section start
const SECTION_START: &str = "# === ";
/// Marker suffix for managed section start
const SECTION_START_SUFFIX: &str = " (managed by bkt) ===";
/// Marker prefix for managed section end
const SECTION_END: &str = "# === END ";
/// Marker suffix for managed section end
const SECTION_END_SUFFIX: &str = " ===";

/// Types of managed sections in the Containerfile
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// System packages installed via dnf
    SystemPackages,
    /// COPR repository enablement
    CoprRepos,
    /// Host shims (COPY and symlinks)
    HostShims,
    /// Kernel arguments (rpm-ostree kargs)
    KernelArguments,
    /// Systemd unit configuration
    SystemdUnits,
}

impl Section {
    /// Get the section name as it appears in markers
    pub fn marker_name(&self) -> &'static str {
        match self {
            Section::SystemPackages => "SYSTEM_PACKAGES",
            Section::CoprRepos => "COPR_REPOS",
            Section::HostShims => "HOST_SHIMS",
            Section::KernelArguments => "KERNEL_ARGUMENTS",
            Section::SystemdUnits => "SYSTEMD_UNITS",
        }
    }

    /// Get the start marker for this section
    pub fn start_marker(&self) -> String {
        format!(
            "{}{}{}",
            SECTION_START,
            self.marker_name(),
            SECTION_START_SUFFIX
        )
    }

    /// Get the end marker for this section
    pub fn end_marker(&self) -> String {
        format!(
            "{}{}{}",
            SECTION_END,
            self.marker_name(),
            SECTION_END_SUFFIX
        )
    }

    /// Parse a section name from a start marker line
    fn from_start_marker(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if !trimmed.starts_with(SECTION_START) || !trimmed.ends_with(SECTION_START_SUFFIX) {
            return None;
        }

        let name = &trimmed[SECTION_START.len()..trimmed.len() - SECTION_START_SUFFIX.len()];
        match name {
            "SYSTEM_PACKAGES" => Some(Section::SystemPackages),
            "COPR_REPOS" => Some(Section::CoprRepos),
            "HOST_SHIMS" => Some(Section::HostShims),
            "KERNEL_ARGUMENTS" => Some(Section::KernelArguments),
            "SYSTEMD_UNITS" => Some(Section::SystemdUnits),
            _ => None,
        }
    }
}

/// A managed block in the Containerfile
#[derive(Debug, Clone)]
pub struct ManagedBlock {
    /// The section type
    pub section: Section,
    /// The content between markers (not including markers themselves)
    pub content: Vec<String>,
    /// Line number where this block starts (0-indexed)
    pub start_line: usize,
    /// Line number where this block ends (0-indexed, inclusive)
    pub end_line: usize,
}

/// Represents either a managed section or unmanaged content
#[derive(Debug, Clone)]
enum ContainerfileSegment {
    /// Unmanaged lines that should be preserved as-is
    Unmanaged(Vec<String>),
    /// A managed section that can be regenerated
    Managed(ManagedBlock),
}

/// Editor for Containerfile with managed sections
#[derive(Debug)]
pub struct ContainerfileEditor {
    /// Path to the Containerfile
    path: PathBuf,
    /// Parsed segments of the file
    segments: Vec<ContainerfileSegment>,
}

impl ContainerfileEditor {
    /// Load and parse a Containerfile
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read Containerfile at {}", path.display()))?;

        Self::parse(path.to_path_buf(), &content)
    }

    /// Parse Containerfile content into segments
    fn parse(path: PathBuf, content: &str) -> Result<Self> {
        let lines: Vec<&str> = content.lines().collect();
        let mut segments = Vec::new();
        let mut current_unmanaged: Vec<String> = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];

            // Check if this is a section start marker
            if let Some(section) = Section::from_start_marker(line) {
                // Save any accumulated unmanaged content
                if !current_unmanaged.is_empty() {
                    segments.push(ContainerfileSegment::Unmanaged(std::mem::take(
                        &mut current_unmanaged,
                    )));
                }

                // Find the end marker
                let start_line = i;
                let end_marker = section.end_marker();
                let mut content_lines = Vec::new();
                i += 1;

                while i < lines.len() {
                    if lines[i].trim() == end_marker {
                        break;
                    }
                    content_lines.push(lines[i].to_string());
                    i += 1;
                }

                if i >= lines.len() {
                    bail!(
                        "Unclosed managed section {} starting at line {}",
                        section.marker_name(),
                        start_line + 1
                    );
                }

                segments.push(ContainerfileSegment::Managed(ManagedBlock {
                    section,
                    content: content_lines,
                    start_line,
                    end_line: i,
                }));

                i += 1; // Skip the end marker
            } else {
                current_unmanaged.push(line.to_string());
                i += 1;
            }
        }

        // Save any remaining unmanaged content
        if !current_unmanaged.is_empty() {
            segments.push(ContainerfileSegment::Unmanaged(current_unmanaged));
        }

        Ok(Self { path, segments })
    }

    /// Update a managed section with new content
    pub fn update_section(&mut self, section: Section, content: Vec<String>) {
        for segment in &mut self.segments {
            if let ContainerfileSegment::Managed(block) = segment
                && block.section == section
            {
                block.content = content;
                return;
            }
        }

        // Section not found - warn the user
        eprintln!(
            "Warning: managed section {} not found in Containerfile {}",
            section.marker_name(),
            self.path.display()
        );
    }

    /// Check if a section exists in the Containerfile
    pub fn has_section(&self, section: Section) -> bool {
        self.segments.iter().any(
            |seg| matches!(seg, ContainerfileSegment::Managed(block) if block.section == section),
        )
    }

    /// Get the current content of a section
    pub fn get_section_content(&self, section: Section) -> Option<&[String]> {
        for segment in &self.segments {
            if let ContainerfileSegment::Managed(block) = segment
                && block.section == section
            {
                return Some(&block.content);
            }
        }
        None
    }

    /// Write the Containerfile back to disk
    pub fn write(&self) -> Result<()> {
        let content = self.render();
        fs::write(&self.path, content)
            .with_context(|| format!("Failed to write Containerfile at {}", self.path.display()))?;
        Ok(())
    }

    /// Render the Containerfile to a string
    pub fn render(&self) -> String {
        let mut lines = Vec::new();

        for segment in &self.segments {
            match segment {
                ContainerfileSegment::Unmanaged(content) => {
                    lines.extend(content.iter().cloned());
                }
                ContainerfileSegment::Managed(block) => {
                    lines.push(block.section.start_marker());
                    lines.extend(block.content.iter().cloned());
                    lines.push(block.section.end_marker());
                }
            }
        }

        // Ensure file ends with newline (only if there's content)
        let mut result = lines.join("\n");
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result
    }
}

/// Generate the SYSTEM_PACKAGES section content from a manifest
pub fn generate_system_packages(packages: &[String]) -> Vec<String> {
    if packages.is_empty() {
        return vec!["# No packages configured".to_string()];
    }

    let mut sorted_packages: Vec<_> = packages.iter().collect();
    sorted_packages.sort();

    let mut lines = Vec::new();
    lines.push("RUN dnf install -y \\".to_string());

    for pkg in sorted_packages.iter() {
        lines.push(format!("    {} \\", pkg));
    }
    lines.push("    && dnf clean all".to_string());

    lines
}

/// Generate the COPR_REPOS section content from a list of COPR repos
pub fn generate_copr_repos(repos: &[String]) -> Vec<String> {
    if repos.is_empty() {
        return vec!["# No COPR repositories configured".to_string()];
    }

    let mut sorted_repos: Vec<_> = repos.iter().collect();
    sorted_repos.sort();

    let mut lines = Vec::new();
    lines.push("RUN set -eu; \\".to_string());

    for (i, repo) in sorted_repos.iter().enumerate() {
        if i < sorted_repos.len() - 1 {
            lines.push(format!("    dnf copr enable -y {}; \\", repo));
        } else {
            lines.push(format!("    dnf copr enable -y {}", repo));
        }
    }

    lines
}

/// Generate the HOST_SHIMS section content from a list of shims.
///
/// Creates shell commands that:
/// 1. Create the shim and bin directories
/// 2. Create each shim script using base64 encoding (avoids heredoc parsing issues)
/// 3. Make shims executable
/// 4. Create symlinks in PATH
pub fn generate_host_shims(shims: &[Shim]) -> Vec<String> {
    if shims.is_empty() {
        return vec!["# No host shims configured".to_string()];
    }

    let mut sorted_shims: Vec<_> = shims.iter().collect();
    sorted_shims.sort_by(|a, b| a.name.cmp(&b.name));

    let shims_dir = "/usr/etc/skel/.local/toolbox/shims";
    let bin_dir = "/usr/etc/skel/.local/bin";

    let mut lines = Vec::new();
    lines.push("RUN set -eu; \\".to_string());
    lines.push(format!("    mkdir -p {} {}; \\", shims_dir, bin_dir));

    for (i, shim) in sorted_shims.iter().enumerate() {
        let host_cmd = shim.host_cmd();
        // Use shlex for proper POSIX-compliant shell quoting
        let quoted = shlex::try_quote(host_cmd).unwrap_or_else(|_| host_cmd.into());

        // Generate the shim script content
        let script_content = format!("#!/bin/bash\nexec flatpak-spawn --host {} \"$@\"\n", quoted);

        // Base64 encode to avoid heredoc parsing issues in Dockerfile
        let encoded = BASE64_STANDARD.encode(script_content.as_bytes());

        let is_last = i == sorted_shims.len() - 1;

        // Use base64 encoding instead of heredoc to avoid Dockerfile parsing issues
        // All commands on same logical line with && to stay in RUN context
        lines.push(format!(
            "    echo '{}' | base64 -d > {}/{} && \\",
            encoded, shims_dir, shim.name
        ));
        lines.push(format!("    chmod 0755 {}/{} && \\", shims_dir, shim.name));

        // Last symlink doesn't need continuation backslash
        if is_last {
            lines.push(format!(
                "    ln -sf ../toolbox/shims/{} {}/{}",
                shim.name, bin_dir, shim.name
            ));
        } else {
            lines.push(format!(
                "    ln -sf ../toolbox/shims/{} {}/{}; \\",
                shim.name, bin_dir, shim.name
            ));
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_markers() {
        assert_eq!(
            Section::SystemPackages.start_marker(),
            "# === SYSTEM_PACKAGES (managed by bkt) ==="
        );
        assert_eq!(
            Section::SystemPackages.end_marker(),
            "# === END SYSTEM_PACKAGES ==="
        );
    }

    #[test]
    fn test_parse_start_marker() {
        assert_eq!(
            Section::from_start_marker("# === SYSTEM_PACKAGES (managed by bkt) ==="),
            Some(Section::SystemPackages)
        );
        assert_eq!(
            Section::from_start_marker("# === COPR_REPOS (managed by bkt) ==="),
            Some(Section::CoprRepos)
        );
        assert_eq!(Section::from_start_marker("# Not a marker"), None);
    }

    #[test]
    fn test_parse_containerfile_with_sections() {
        let content = r#"FROM fedora:41

# Some setup
RUN echo "setup"

# === SYSTEM_PACKAGES (managed by bkt) ===
RUN dnf install -y \
    htop \
    vim
# === END SYSTEM_PACKAGES ===

# More content
COPY . /app
"#;

        let editor = ContainerfileEditor::parse(PathBuf::from("test"), content).unwrap();

        assert!(editor.has_section(Section::SystemPackages));
        assert!(!editor.has_section(Section::CoprRepos));

        let pkg_content = editor.get_section_content(Section::SystemPackages).unwrap();
        assert_eq!(pkg_content.len(), 3);
    }

    #[test]
    fn test_update_section() {
        let content = r#"FROM fedora:41

# === SYSTEM_PACKAGES (managed by bkt) ===
RUN dnf install -y htop
# === END SYSTEM_PACKAGES ===

COPY . /app
"#;

        let mut editor = ContainerfileEditor::parse(PathBuf::from("test"), content).unwrap();

        let new_content = generate_system_packages(&["htop".to_string(), "vim".to_string()]);
        editor.update_section(Section::SystemPackages, new_content);

        let rendered = editor.render();
        assert!(rendered.contains("htop"));
        assert!(rendered.contains("vim"));
    }

    #[test]
    fn test_generate_system_packages() {
        let packages = vec!["vim".to_string(), "htop".to_string(), "curl".to_string()];
        let lines = generate_system_packages(&packages);

        assert!(lines[0].contains("dnf install"));
        // Should be sorted alphabetically
        assert!(lines[1].contains("curl"));
        assert!(lines[2].contains("htop"));
        assert!(lines[3].contains("vim"));
    }

    #[test]
    fn test_generate_copr_repos() {
        let repos = vec!["atim/starship".to_string(), "someone/thing".to_string()];
        let lines = generate_copr_repos(&repos);

        assert!(lines[0].contains("set -eu"));
        // Should be sorted alphabetically
        assert!(lines[1].contains("atim/starship"));
        assert!(lines[2].contains("someone/thing"));
    }

    #[test]
    fn test_render_preserves_unmanaged() {
        let content = r#"FROM fedora:41

# Custom setup that should be preserved
RUN echo "custom"

# === SYSTEM_PACKAGES (managed by bkt) ===
RUN dnf install -y htop
# === END SYSTEM_PACKAGES ===

# More custom content
COPY . /app
"#;

        let editor = ContainerfileEditor::parse(PathBuf::from("test"), content).unwrap();
        let rendered = editor.render();

        assert!(rendered.contains("Custom setup that should be preserved"));
        assert!(rendered.contains("More custom content"));
        assert!(rendered.contains("FROM fedora:41"));
        assert!(rendered.contains("COPY . /app"));
    }

    #[test]
    fn test_generate_system_packages_empty() {
        let packages: Vec<String> = vec![];
        let lines = generate_system_packages(&packages);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "# No packages configured");
    }

    #[test]
    fn test_generate_system_packages_format() {
        // Verify exact output format including trailing backslashes
        let packages = vec!["pkg1".to_string(), "pkg2".to_string()];
        let lines = generate_system_packages(&packages);

        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "RUN dnf install -y \\");
        assert_eq!(lines[1], "    pkg1 \\");
        assert_eq!(lines[2], "    pkg2 \\");
        assert_eq!(lines[3], "    && dnf clean all");
    }

    #[test]
    fn test_generate_copr_repos_empty() {
        let repos: Vec<String> = vec![];
        let lines = generate_copr_repos(&repos);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "# No COPR repositories configured");
    }

    #[test]
    fn test_parse_unclosed_section_error() {
        let content = r#"FROM fedora:41

# === SYSTEM_PACKAGES (managed by bkt) ===
RUN dnf install -y htop
# Missing end marker

COPY . /app
"#;

        let result = ContainerfileEditor::parse(PathBuf::from("test"), content);
        assert!(result.is_err());

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("Unclosed managed section"));
        assert!(err_msg.contains("SYSTEM_PACKAGES"));
    }

    // =========================================================================
    // HOST_SHIMS tests
    // =========================================================================

    fn sample_shim(name: &str) -> Shim {
        Shim {
            name: name.to_string(),
            host: None,
        }
    }

    fn sample_shim_with_host(name: &str, host: &str) -> Shim {
        Shim {
            name: name.to_string(),
            host: Some(host.to_string()),
        }
    }

    #[test]
    fn test_generate_host_shims_empty() {
        let shims: Vec<Shim> = vec![];
        let lines = generate_host_shims(&shims);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "# No host shims configured");
    }

    #[test]
    fn test_generate_host_shims_single() {
        let shims = vec![sample_shim("bootc")];
        let lines = generate_host_shims(&shims);

        // Check structure
        assert!(lines[0].contains("set -eu"));
        assert!(lines[1].contains("mkdir -p"));
        assert!(lines[1].contains("/usr/etc/skel/.local/toolbox/shims"));
        assert!(lines[1].contains("/usr/etc/skel/.local/bin"));

        // Check shim creation uses base64 encoding
        let content = lines.join("\n");
        assert!(content.contains("base64 -d > /usr/etc/skel/.local/toolbox/shims/bootc"));
        assert!(content.contains("chmod 0755"));
        assert!(content.contains("ln -sf ../toolbox/shims/bootc /usr/etc/skel/.local/bin/bootc"));

        // Verify the base64 decodes correctly
        let expected_script = "#!/bin/bash\nexec flatpak-spawn --host bootc \"$@\"\n";
        let encoded = BASE64_STANDARD.encode(expected_script.as_bytes());
        assert!(content.contains(&encoded));
    }

    #[test]
    fn test_generate_host_shims_multiple() {
        let shims = vec![
            sample_shim("systemctl"),
            sample_shim("bootc"),
            sample_shim("podman"),
        ];
        let lines = generate_host_shims(&shims);
        let content = lines.join("\n");

        // Should be sorted alphabetically
        let bootc_pos = content.find("shims/bootc").unwrap();
        let podman_pos = content.find("shims/podman").unwrap();
        let systemctl_pos = content.find("shims/systemctl").unwrap();

        assert!(bootc_pos < podman_pos);
        assert!(podman_pos < systemctl_pos);

        // All three shims should have base64-encoded scripts
        // Verify by checking the encoded strings are present
        for cmd in ["bootc", "podman", "systemctl"] {
            let expected_script =
                format!("#!/bin/bash\nexec flatpak-spawn --host {} \"$@\"\n", cmd);
            let encoded = BASE64_STANDARD.encode(expected_script.as_bytes());
            assert!(
                content.contains(&encoded),
                "Missing encoded script for {}",
                cmd
            );
        }
    }

    #[test]
    fn test_generate_host_shims_with_host_override() {
        let shims = vec![sample_shim_with_host("docker", "podman")];
        let lines = generate_host_shims(&shims);
        let content = lines.join("\n");

        // The shim name should be "docker" but it should call "podman" on host
        assert!(content.contains("shims/docker"));
        assert!(content.contains("/bin/docker"));

        // Verify the base64 encodes the correct host command (podman, not docker)
        let expected_script = "#!/bin/bash\nexec flatpak-spawn --host podman \"$@\"\n";
        let encoded = BASE64_STANDARD.encode(expected_script.as_bytes());
        assert!(
            content.contains(&encoded),
            "Should contain base64-encoded script calling podman"
        );
    }

    #[test]
    fn test_generate_host_shims_no_trailing_backslash() {
        let shims = vec![sample_shim("bootc")];
        let lines = generate_host_shims(&shims);

        // Last line should NOT end with a backslash continuation
        let last_line = lines.last().unwrap();
        assert!(
            !last_line.ends_with("\\"),
            "Last line should not end with backslash: {}",
            last_line
        );
    }
}

/// Generate the KERNEL_ARGUMENTS section content from a manifest
pub fn generate_kernel_arguments(manifest: &SystemConfigManifest) -> Vec<String> {
    let kargs = match &manifest.kargs {
        Some(k) => k,
        None => return vec!["# No kernel arguments configured".to_string()],
    };

    if kargs.append.is_empty() && kargs.remove.is_empty() {
        return vec!["# No kernel arguments configured".to_string()];
    }

    let mut args = Vec::new();
    for arg in &kargs.remove {
        args.push(format!("--delete={}", arg));
    }
    for arg in &kargs.append {
        args.push(format!("--append={}", arg));
    }

    let mut lines = Vec::new();
    lines.push("RUN rpm-ostree kargs \\".to_string());

    for (i, arg) in args.iter().enumerate() {
        if i < args.len() - 1 {
            lines.push(format!("    {} \\", arg));
        } else {
            lines.push(format!("    {}", arg));
        }
    }

    lines
}

/// Generate the SYSTEMD_UNITS section content from a manifest
pub fn generate_systemd_units(manifest: &SystemConfigManifest) -> Vec<String> {
    let systemd = match &manifest.systemd {
        Some(s) => s,
        None => return vec!["# No systemd units configured".to_string()],
    };

    if systemd.enable.is_empty() && systemd.disable.is_empty() && systemd.mask.is_empty() {
        return vec!["# No systemd units configured".to_string()];
    }

    let mut commands = Vec::new();

    if !systemd.enable.is_empty() {
        let units = systemd.enable.join(" ");
        commands.push(format!("systemctl enable {}", units));
    }

    if !systemd.disable.is_empty() {
        let units = systemd.disable.join(" ");
        commands.push(format!("systemctl disable {}", units));
    }

    if !systemd.mask.is_empty() {
        let units = systemd.mask.join(" ");
        commands.push(format!("systemctl mask {}", units));
    }

    let mut lines = Vec::new();
    lines.push("RUN set -eu; \\".to_string());

    for (i, cmd) in commands.iter().enumerate() {
        if i < commands.len() - 1 {
            lines.push(format!("    {}; \\", cmd));
        } else {
            lines.push(format!("    {}", cmd));
        }
    }

    lines
}
