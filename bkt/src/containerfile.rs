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

use crate::manifest::ExternalReposManifest;
use crate::manifest::Shim;
use crate::manifest::external_repos::LayerGroup;
use crate::manifest::image_config::{FileCopy, ImageConfigManifest, ImageModule};
use crate::manifest::system_config::SystemConfigManifest;
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use bkt_common::manifest::{InstallConfig, Upstream, UpstreamManifest};
use std::cmp::Ordering;
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
const HEADER_WIDTH: usize = 79;
const LINE_CONT: &str = "\\";

/// Types of managed sections in the Containerfile
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// System packages installed via dnf
    SystemPackages,
    /// COPR repository enablement
    CoprRepos,
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

/// Input data for full Containerfile generation.
pub struct ContainerfileGeneratorInput {
    pub external_repos: ExternalReposManifest,
    pub upstreams: UpstreamManifest,
    pub packages: Vec<String>,
    pub copr_repos: Vec<String>,
    pub system_config: SystemConfigManifest,
    pub image_config: ImageConfigManifest,
    pub shims: Vec<Shim>,
    pub has_external_rpms: bool,
}

/// Generate the full Containerfile from manifests.
pub fn generate_full_containerfile(input: &ContainerfileGeneratorInput) -> String {
    let mut lines = Vec::new();

    emit_tools_stage(&mut lines);
    emit_base_stage(&mut lines);
    emit_dl_stages(&mut lines, &input.external_repos);
    emit_install_stages(&mut lines, &input.external_repos);
    emit_bundled_stage(&mut lines, &input.external_repos);
    emit_fetch_stages(&mut lines, &input.upstreams);
    emit_script_stages(&mut lines, &input.upstreams);
    emit_wrapper_build_stage(&mut lines, &input.image_config);
    emit_collect_config(&mut lines, &input.image_config);
    emit_collect_outputs(&mut lines, &input.upstreams, &input.image_config);
    emit_image_assembly(&mut lines, input);

    let mut result = lines.join("\n");
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn emit_managed_section(lines: &mut Vec<String>, section: Section, content: &[String]) {
    lines.push(section.start_marker());
    lines.extend(content.iter().cloned());
    lines.push(section.end_marker());
}

fn section_header(title: &str) -> String {
    let dash = '\u{2500}';
    let mut line = String::new();
    line.push('#');
    line.push(' ');
    line.push(dash);
    line.push(dash);
    line.push(' ');
    line.push_str(title);
    line.push(' ');

    while line.chars().count() < HEADER_WIDTH {
        line.push(dash);
    }
    line
}

fn emit_tools_stage(lines: &mut Vec<String>) {
    lines.push(section_header("Tools stage"));
    lines.push("# Static musl binary for build-time operations (built by CI)".to_string());
    lines.push("FROM scratch AS tools".to_string());
    lines.push("COPY scripts/bkt-build /bkt-build".to_string());
}

fn emit_base_stage(lines: &mut Vec<String>) {
    lines.push("".to_string());
    lines.push(section_header("Base stage (repos configured)"));
    lines.push("FROM ghcr.io/ublue-os/bazzite-gnome:stable AS base".to_string());
    lines.push("COPY --from=tools /bkt-build /usr/bin/bkt-build".to_string());
    lines.push("COPY manifests/external-repos.json /tmp/external-repos.json".to_string());
    lines.push(format!("RUN set -eu; {}", LINE_CONT));
    lines.push(format!(
        "    mkdir -p /var/opt /var/usrlocal/bin; {}",
        LINE_CONT
    ));
    lines.push("    bkt-build setup-repos".to_string());
}

/// Convert a repo name to a Dockerfile ARG name for cache busting.
/// e.g. "microsoft-edge" -> "CACHE_EPOCH_MICROSOFT_EDGE"
fn cache_arg_name(repo_name: &str) -> String {
    let sanitized: String = repo_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("CACHE_EPOCH_{sanitized}")
}

fn emit_dl_stages(lines: &mut Vec<String>, repos: &ExternalReposManifest) {
    lines.push("".to_string());
    lines.push(section_header(
        "RPM download stages (parallel, each downloads from one external repo)",
    ));
    lines.push("".to_string());

    for (idx, repo) in repos.repos.iter().enumerate() {
        if idx > 0 {
            lines.push("".to_string());
        }
        lines.push(format!("FROM base AS dl-{}", repo.name));
        lines.push(format!("ARG {}=0", cache_arg_name(&repo.name)));
        lines.push(format!("RUN bkt-build download-rpms {}", repo.name));
    }
}

/// Emit per-package install stages that extract RPMs without DB/scripts.
/// Each stage runs `rpm -i --nodb --noscripts --nodeps` and handles /opt relocation.
fn emit_install_stages(lines: &mut Vec<String>, repos: &ExternalReposManifest) {
    lines.push("".to_string());
    lines.push(section_header(
        "RPM install stages (per-package extraction with /opt relocation)",
    ));
    lines.push("".to_string());

    for (idx, repo) in repos.repos.iter().enumerate() {
        if idx > 0 {
            lines.push("".to_string());
        }
        lines.push(format!("FROM base AS install-{}", repo.name));
        lines.push(format!("COPY --from=dl-{} /rpms/ /tmp/rpms/", repo.name));

        if let Some(opt_path) = &repo.opt_path {
            // Repo installs to /opt — extract and relocate
            lines.push(format!("RUN set -eu; {}", LINE_CONT));
            lines.push(format!(
                "    rpm -i --nodb --noscripts --nodeps /tmp/rpms/*.rpm; {}",
                LINE_CONT
            ));
            lines.push(format!("    mkdir -p /usr/lib/opt; {}", LINE_CONT));
            lines.push(format!(
                "    if [ -d /opt/{opt} ]; then cp -a /opt/{opt}/. /usr/lib/opt/{opt}/; rm -rf /opt/{opt}; fi; {cont}",
                opt = opt_path,
                cont = LINE_CONT
            ));
            lines.push("    rm -rf /tmp/rpms".to_string());
        } else {
            // No /opt relocation needed
            lines.push(
                "RUN rpm -i --nodb --noscripts --nodeps /tmp/rpms/*.rpm && rm -rf /tmp/rpms"
                    .to_string(),
            );
        }
    }
}

/// Emit a merged stage for bundled packages (if any exist).
/// This stage combines outputs from individual install-* stages into one layer.
fn emit_bundled_stage(lines: &mut Vec<String>, repos: &ExternalReposManifest) {
    let bundled: Vec<_> = repos
        .repos
        .iter()
        .filter(|r| r.layer_group == LayerGroup::Bundled)
        .collect();

    if bundled.is_empty() {
        return;
    }

    lines.push("".to_string());
    lines.push(section_header(
        "Bundled packages merged stage (reduces deployment layer count)",
    ));
    lines.push("".to_string());
    lines.push("FROM scratch AS install-bundled".to_string());
    for repo in &bundled {
        lines.push(format!("COPY --from=install-{} / /", repo.name));
    }
}

/// Emit COPY --link instructions from install-* stages into final image.
/// Independent packages get their own layer; bundled packages share one layer.
fn emit_install_copies(lines: &mut Vec<String>, repos: &ExternalReposManifest) {
    let independent: Vec<_> = repos
        .repos
        .iter()
        .filter(|r| r.layer_group == LayerGroup::Independent)
        .collect();
    let has_bundled = repos
        .repos
        .iter()
        .any(|r| r.layer_group == LayerGroup::Bundled);

    lines.push(
        "# Import installed files from install-* stages (COPY --link for layer independence)"
            .to_string(),
    );

    // Independent packages get their own COPY --link
    for repo in &independent {
        lines.push(format!("COPY --link --from=install-{} / /", repo.name));
    }

    // Bundled packages share one merged layer
    if has_bundled {
        lines.push("COPY --link --from=install-bundled / /".to_string());
    }
}

/// Emit RPM database finalization after all file payloads are in place.
fn emit_rpm_db_finalization(lines: &mut Vec<String>, repos: &ExternalReposManifest) {
    if repos.repos.is_empty() {
        return;
    }

    lines.push("# Finalize RPM database for external packages".to_string());
    // Copy RPMs to separate dirs to avoid filename collisions
    for repo in &repos.repos {
        lines.push(format!(
            "COPY --from=dl-{} /rpms/ /tmp/rpms-{}/",
            repo.name, repo.name
        ));
    }

    lines.push(format!("RUN set -eu; {}", LINE_CONT));
    for repo in &repos.repos {
        lines.push(format!(
            "    rpm -i --justdb --nodeps /tmp/rpms-{}/*.rpm; {}",
            repo.name, LINE_CONT
        ));
    }
    lines.push(format!("    ldconfig; {}", LINE_CONT));

    // Cleanup
    let cleanup_parts: Vec<String> = repos
        .repos
        .iter()
        .map(|r| format!("/tmp/rpms-{}", r.name))
        .collect();
    lines.push(format!("    rm -rf {}", cleanup_parts.join(" ")));
}

fn emit_fetch_stages(lines: &mut Vec<String>, upstreams: &UpstreamManifest) {
    lines.push("".to_string());
    lines.push(section_header(
        "Upstream fetch stages (parallel, each installs one upstream entry)",
    ));
    lines.push("".to_string());

    for (idx, upstream) in ordered_upstreams(upstreams, |u| {
        matches!(
            u.install,
            Some(InstallConfig::Binary { .. }) | Some(InstallConfig::Archive { .. })
        )
    })
    .into_iter()
    .enumerate()
    {
        if idx > 0 {
            lines.push("".to_string());
        }
        lines.push(format!("FROM base AS fetch-{}", upstream.name));
        lines.push("COPY upstream/manifest.json /tmp/upstream-manifest.json".to_string());
        lines.push(format!("RUN bkt-build fetch {}", upstream.name));
    }
}

fn emit_script_stages(lines: &mut Vec<String>, upstreams: &UpstreamManifest) {
    lines.push("".to_string());
    lines.push(section_header(
        "Bespoke build stages (parallel, for script-type installs)",
    ));
    lines.push("".to_string());

    for (idx, upstream) in ordered_upstreams(upstreams, |u| {
        matches!(u.install, Some(InstallConfig::Script { .. }))
    })
    .into_iter()
    .enumerate()
    {
        let install = match &upstream.install {
            Some(InstallConfig::Script { .. }) => upstream.install.as_ref().unwrap(),
            _ => continue,
        };

        if idx > 0 {
            lines.push("".to_string());
        }

        let stage = script_stage_name(upstream, install);
        let script_lines = script_build_script(install);

        let outputs = upstream_outputs(Some(install));

        lines.push(format!("FROM base AS {}", stage));
        lines.push("COPY upstream/manifest.json /tmp/upstream-manifest.json".to_string());
        if let Some(script) = script_lines {
            lines.push("RUN <<'EOF'".to_string());
            lines.extend(script.into_iter());

            // Collect outputs into /out/ for single-layer COPY (RFC-0050)
            if !outputs.is_empty() {
                lines.push("# Collect outputs for single-layer COPY".to_string());
                // Gather unique parent directories
                let mut parents: Vec<String> = outputs
                    .iter()
                    .map(|o| {
                        if o.ends_with('/') {
                            format!("/out{}", o)
                        } else {
                            let path = std::path::Path::new(o.as_str());
                            format!(
                                "/out{}",
                                path.parent().unwrap_or(std::path::Path::new("/")).display()
                            )
                        }
                    })
                    .collect();
                parents.sort();
                parents.dedup();
                lines.push(format!("mkdir -p {}", parents.join(" ")));

                for output in &outputs {
                    if output.ends_with('/') {
                        lines.push(format!("cp -r {} /out{}", output, output));
                    } else {
                        lines.push(format!("cp {} /out{}", output, output));
                    }
                }
            }

            lines.push("EOF".to_string());
        } else {
            lines.push(format!("RUN bkt-build fetch {}", upstream.name));
        }
    }
}

/// Emit the `FROM scratch AS collect-config` stage.
///
/// This stage assembles all static configuration files via COPY instructions
/// only (no RUN — FROM scratch has no shell). The image stage imports the
/// result with a single `COPY --from=collect-config / /`.
fn emit_collect_config(lines: &mut Vec<String>, image_config: &ImageConfigManifest) {
    lines.push("".to_string());
    lines.push(section_header("Config collector (parallel, FROM scratch)"));
    lines.push("FROM scratch AS collect-config".to_string());

    for module in &image_config.modules {
        match module {
            ImageModule::Files { files, .. } => {
                if let Some(comment) = module.comment() {
                    lines.push("".to_string());
                    for line in comment.split('\n') {
                        lines.push(format!("# {}", line));
                    }
                }
                emit_copy_files(lines, files);
            }
            ImageModule::OptionalFeature { src, staging, .. } => {
                if let Some(comment) = module.comment() {
                    lines.push("".to_string());
                    for line in comment.split('\n') {
                        lines.push(format!("# {}", line));
                    }
                }
                lines.push(format!("COPY {} {}", src, staging));
            }
            // Run and SystemdEnable modules need a shell — handled in image stage
            ImageModule::Run { .. } | ImageModule::SystemdEnable { .. } => {}
            // Wrapper modules are handled separately (built binaries copied in)
            ImageModule::Wrapper { .. } => {}
        }
    }
}

/// Collect all RUN-requiring operations from image modules and emit them
/// as a single consolidated `RUN set -eu; \` block in the image stage.
///
/// This replaces the many individual RUN instructions that were previously
/// emitted by `emit_image_modules()`.
fn emit_consolidated_run(
    lines: &mut Vec<String>,
    image_config: &ImageConfigManifest,
    shims: &[Shim],
) {
    let mut commands: Vec<String> = Vec::new();

    for module in &image_config.modules {
        match module {
            ImageModule::Run { commands: cmds, .. } if module.name() != "font-cache" => {
                commands.extend(cmds.iter().cloned());
            }
            ImageModule::Files {
                pre_run, post_run, ..
            } => {
                commands.extend(pre_run.iter().cloned());
                commands.extend(post_run.iter().cloned());
            }
            ImageModule::SystemdEnable {
                scope,
                unit,
                target,
                ..
            } => {
                let wants_dir = format!("/usr/lib/systemd/{}/{}.wants", scope, target);
                commands.push(format!("mkdir -p {}", wants_dir));
                commands.push(format!("ln -sf ../{} {}/{}", unit, wants_dir, unit));
            }
            ImageModule::OptionalFeature {
                staging_pre_run, ..
            } => {
                commands.extend(staging_pre_run.iter().cloned());
            }
            _ => {}
        }
    }

    commands.extend(generate_host_shim_commands(shims));

    for module in &image_config.modules {
        if let ImageModule::OptionalFeature {
            arg,
            staging,
            dest,
            post_install,
            ..
        } = module
        {
            let mut conditional = format!(
                "if [ \"${{{}}}\" = \"1\" ]; then install -Dpm0644 {} {}",
                arg, staging, dest
            );
            for cmd in post_install {
                conditional.push_str("; ");
                conditional.push_str(cmd);
            }
            conditional.push_str("; fi");
            commands.push(conditional);
        }
    }

    if commands.is_empty() {
        return;
    }

    lines.push("# Post-overlay setup: chmod, symlinks, mkdir, kargs, staging dirs".to_string());
    lines.push(format!("RUN set -eu; {}", LINE_CONT));
    for (idx, cmd) in commands.iter().enumerate() {
        let is_last = idx == commands.len() - 1;
        if let Some((left, right)) = split_redirect_command(cmd) {
            lines.push(format!("    {} {}", left, LINE_CONT));
            if is_last {
                lines.push(format!("    > {}", right));
            } else {
                lines.push(format!("    > {}; {}", right, LINE_CONT));
            }
        } else if is_last {
            lines.push(format!("    {}", cmd));
        } else {
            lines.push(format!("    {}; {}", cmd, LINE_CONT));
        }
    }
}

/// Emit ARG-gated optional feature conditionals in the image stage.
///
/// Each optional feature gets its own ARG + RUN if [...] block because
/// the ARG must precede the RUN that references it.
/// Emit the `FROM scratch AS collect-outputs` stage.
///
/// Gathers upstream fetch/build outputs, config, and wrappers into a single
/// stage so the final image can import them with one COPY instruction.
/// This reduces the OCI layer count to stay under btrfs hardlink limits
/// in containers/storage.
fn emit_collect_outputs(
    lines: &mut Vec<String>,
    upstreams: &UpstreamManifest,
    image_config: &ImageConfigManifest,
) {
    lines.push("".to_string());
    lines.push(section_header(
        "Output collector (merges upstream + config + wrappers into single layer)",
    ));
    lines.push("FROM scratch AS collect-outputs".to_string());
    lines.push("".to_string());

    // Upstream fetch outputs (binary/archive)
    let fetch_upstreams = ordered_upstreams(upstreams, |u| {
        matches!(
            u.install,
            Some(InstallConfig::Binary { .. }) | Some(InstallConfig::Archive { .. })
        )
    });
    for upstream in &fetch_upstreams {
        let install = upstream.install.as_ref();
        let outputs = upstream_outputs(install);
        for output in outputs {
            lines.push(format!(
                "COPY --from=fetch-{} {} {}",
                upstream.name, output, output
            ));
        }
    }

    // Upstream script/build outputs (collected in /out/)
    let script_upstreams = ordered_upstreams(upstreams, |u| {
        matches!(u.install, Some(InstallConfig::Script { .. }))
    });
    for upstream in &script_upstreams {
        let install = upstream.install.as_ref();
        let outputs = upstream_outputs(install);
        let stage = script_stage_name(upstream, install.unwrap());
        if !outputs.is_empty() {
            lines.push(format!("COPY --from={} /out/ /", stage));
        }
    }

    // Static config from collect-config
    lines.push("COPY --from=collect-config / /".to_string());

    // Wrapper binaries
    let wrappers = image_config.wrappers();
    if !wrappers.is_empty() {
        lines.push("COPY --from=build-wrappers /out/ /".to_string());
    }
}

fn emit_image_assembly(lines: &mut Vec<String>, input: &ContainerfileGeneratorInput) {
    lines.push("".to_string());
    lines.push(section_header("Final image assembly"));
    lines.push("FROM base AS image".to_string());
    lines.push("".to_string());

    let copr = generate_copr_repos(&input.copr_repos);
    emit_managed_section(lines, Section::CoprRepos, &copr);
    lines.push("".to_string());

    let kargs = generate_kernel_arguments(&input.system_config);
    emit_managed_section(lines, Section::KernelArguments, &kargs);
    lines.push("".to_string());

    // Per-package RPM file payloads (COPY --link for layer independence)
    emit_install_copies(lines, &input.external_repos);
    lines.push("".to_string());

    // System packages only (external RPMs handled via install stages)
    let pkgs = generate_system_packages(&input.packages, false);
    emit_managed_section(lines, Section::SystemPackages, &pkgs);
    lines.push("".to_string());

    let units = generate_systemd_units(&input.system_config);
    emit_managed_section(lines, Section::SystemdUnits, &units);
    lines.push("".to_string());

    // tmpfiles for /var/opt symlinks (data-driven from opt_path)
    emit_tmpfiles(lines, &input.external_repos);
    lines.push("".to_string());

    // RPM database finalization (--justdb + ldconfig)
    emit_rpm_db_finalization(lines, &input.external_repos);
    lines.push("".to_string());

    emit_cleanup(lines);
    lines.push("".to_string());

    // Import all upstream outputs, config, and wrappers in a single layer.
    // These are gathered into collect-outputs to minimize OCI layer count
    // (btrfs hardlink limit in containers/storage).
    lines.push(section_header(
        "Upstream outputs + config + wrappers (single layer)",
    ));
    lines.push("COPY --from=collect-outputs / /".to_string());
    lines.push("".to_string());

    // Optional feature ARGs must precede the consolidated RUN
    let mut header_emitted = false;
    for module in &input.image_config.modules {
        if let ImageModule::OptionalFeature { arg, .. } = module {
            if !header_emitted {
                lines.push("".to_string());
                lines.push("# Optional host tweaks (off by default)".to_string());
                header_emitted = true;
            }

            if let Some(comment) = module.comment() {
                lines.push("".to_string());
                for line in comment.split('\n') {
                    lines.push(format!("# {}", line));
                }
            }

            lines.push(format!("ARG {}=0", arg));
        }
    }

    // Consolidated RUN for all post-overlay operations
    emit_consolidated_run(lines, &input.image_config, &input.shims);
    lines.push("".to_string());

    // Font cache after all fonts and config are in place
    lines.push("# Rebuild font cache after all font/icon COPYs and config overlay".to_string());
    lines.push("RUN fc-cache -f".to_string());
    lines.push("".to_string());

    emit_rpm_snapshot(lines);
}

fn emit_tmpfiles(lines: &mut Vec<String>, repos: &ExternalReposManifest) {
    let opt_entries = opt_symlink_entries(repos);

    lines.push(
        "# Create systemd tmpfiles rule to symlink /var/opt contents from /usr/lib/opt".to_string(),
    );
    lines.push(format!("RUN printf '%s\\n' {}", LINE_CONT));
    lines.push(format!(
        "    '# Symlink /opt contents from immutable /usr/lib/opt' {}",
        LINE_CONT
    ));
    for entry in opt_entries {
        lines.push(format!(
            "    'L+ /var/opt/{0} - - - - /usr/lib/opt/{0}' \\",
            entry
        ));
    }
    lines.push("    >/usr/lib/tmpfiles.d/bootc-opt.conf".to_string());
}

fn emit_cleanup(lines: &mut Vec<String>) {
    lines.push(
        "# Clean up build-time artifacts (no longer needed after package install)".to_string(),
    );
    // Note: /tmp/rpms is cleaned up in install stages and db finalization
    lines.push("RUN rm -rf /tmp/external-repos.json /usr/bin/bkt-build".to_string());
}

fn emit_copy_files(lines: &mut Vec<String>, files: &[FileCopy]) {
    for file in files {
        if let Some(comment) = &file.comment {
            lines.push(format!("# {}", comment));
        }
        if let Some(mode) = &file.mode {
            lines.push(format!("COPY --chmod={} {} {}", mode, file.src, file.dest));
        } else {
            lines.push(format!("COPY {} {}", file.src, file.dest));
        }
    }
}

fn split_redirect_command(command: &str) -> Option<(String, String)> {
    let (left, right) = command.split_once(" > ")?;
    Some((left.to_string(), right.to_string()))
}

fn emit_rpm_snapshot(lines: &mut Vec<String>) {
    lines.push("# === RPM VERSION SNAPSHOT ===".to_string());
    lines.push(
        "# Capture installed versions of system packages for OCI label embedding.".to_string(),
    );
    lines.push(
        "# This file is read by the build workflow to create org.wycats.bootc.rpm.versions label."
            .to_string(),
    );
    lines.push(
        "RUN rpm -qa --qf '%{NAME}\\t%{EVR}\\n' | sort > /usr/share/bootc/rpm-versions.txt"
            .to_string(),
    );
    lines.push("# === END RPM VERSION SNAPSHOT ===".to_string());
}

/// Emit a build stage that compiles memory-managed application wrappers.
///
/// The wrapper source is fully derived from manifest data (image-config.json).
/// Each wrapper is a small Rust program (~80 lines, no external deps) that
/// launches an application under systemd-run for memory control.
///
/// This stage uses `rust:slim` as a builder and compiles each wrapper with
/// `rustc` directly (no cargo needed — zero dependencies). The stage runs
/// in parallel with other build stages.
fn emit_wrapper_build_stage(lines: &mut Vec<String>, image_config: &ImageConfigManifest) {
    let wrappers = image_config.wrappers();
    if wrappers.is_empty() {
        return;
    }

    lines.push("".to_string());
    lines.push(section_header(
        "Wrapper build stage (parallel, from manifest)",
    ));
    lines.push("".to_string());
    lines.push("FROM rust:slim AS build-wrappers".to_string());

    // Compile wrappers to their final paths under /out/ for single-layer COPY
    for wrapper in &wrappers {
        let out_path = format!("/out{}", wrapper.output);
        let out_dir = std::path::Path::new(out_path.as_str())
            .parent()
            .unwrap_or(std::path::Path::new("/out"))
            .display();
        lines.push(format!(
            "COPY wrappers/{name}/src/main.rs /tmp/{name}.rs",
            name = wrapper.name
        ));
        lines.push(format!(
            "RUN mkdir -p {dir} && rustc --edition 2021 -O -o {path} /tmp/{name}.rs",
            dir = out_dir,
            path = out_path,
            name = wrapper.name
        ));
    }
}

fn ordered_upstreams<F>(upstreams: &UpstreamManifest, predicate: F) -> Vec<&Upstream>
where
    F: Fn(&Upstream) -> bool,
{
    let mut entries: Vec<&Upstream> = upstreams
        .upstreams
        .iter()
        .filter(|u| predicate(u))
        .collect();
    entries.sort_by(|a, b| {
        let by_date = a.pinned.pinned_at.cmp(&b.pinned.pinned_at);
        if by_date == Ordering::Equal {
            b.name.cmp(&a.name)
        } else {
            by_date
        }
    });
    entries
}

fn script_stage_name(upstream: &Upstream, install: &InstallConfig) -> String {
    match install {
        InstallConfig::Script { stage_name, .. } => stage_name
            .clone()
            .unwrap_or_else(|| format!("build-{}", upstream.name)),
        _ => format!("build-{}", upstream.name),
    }
}

fn script_build_script(install: &InstallConfig) -> Option<Vec<String>> {
    match install {
        InstallConfig::Script { build_script, .. } => build_script.clone(),
        _ => None,
    }
}

fn upstream_outputs(install: Option<&InstallConfig>) -> Vec<String> {
    match install {
        Some(InstallConfig::Binary { install_path }) => vec![install_path.clone()],
        Some(InstallConfig::Archive {
            extract_to,
            outputs,
            ..
        }) => match outputs {
            Some(list) if !list.is_empty() => list.clone(),
            _ => vec![ensure_trailing_slash(extract_to)],
        },
        Some(InstallConfig::Script { outputs, .. }) => outputs.clone().unwrap_or_default(),
        None => Vec::new(),
    }
}

fn ensure_trailing_slash(path: &str) -> String {
    if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{}/", path)
    }
}

fn opt_symlink_entries(repos: &ExternalReposManifest) -> Vec<String> {
    repos
        .repos
        .iter()
        .filter_map(|repo| repo.opt_path.clone())
        .collect()
}

/// Generate the SYSTEM_PACKAGES section content from a manifest.
///
/// When `has_external_rpms` is true, the install line starts with
/// `/tmp/rpms/*.rpm` to install pre-downloaded RPMs from dl-* stages
/// before the Fedora-native packages.
pub fn generate_system_packages(packages: &[String], has_external_rpms: bool) -> Vec<String> {
    if packages.is_empty() && !has_external_rpms {
        return vec!["# No packages configured".to_string()];
    }

    let mut sorted_packages: Vec<_> = packages.iter().collect();
    sorted_packages.sort();

    let mut lines = Vec::new();
    lines.push("RUN dnf install -y \\".to_string());

    if has_external_rpms {
        lines.push("    /tmp/rpms/*.rpm \\".to_string());
    }

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
pub fn generate_host_shim_commands(shims: &[Shim]) -> Vec<String> {
    if shims.is_empty() {
        return Vec::new();
    }

    let mut sorted_shims: Vec<_> = shims.iter().collect();
    sorted_shims.sort_by(|a, b| a.name.cmp(&b.name));

    let shims_dir = "/usr/etc/skel/.local/toolbox/shims";
    let bin_dir = "/usr/etc/skel/.local/bin";

    let mut commands = Vec::new();
    commands.push(format!("mkdir -p {} {}", shims_dir, bin_dir));

    for shim in sorted_shims.iter() {
        let host_cmd = shim.host_cmd();
        // Use shlex for proper POSIX-compliant shell quoting
        let quoted = shlex::try_quote(host_cmd).unwrap_or_else(|_| host_cmd.into());

        // Generate the shim script content
        let script_content = format!("#!/bin/bash\nexec flatpak-spawn --host {} \"$@\"\n", quoted);

        // Base64 encode to avoid heredoc parsing issues in Dockerfile
        let encoded = BASE64_STANDARD.encode(script_content.as_bytes());

        let shim_cmd = format!(
            "echo '{}' | base64 -d > {}/{} && chmod 0755 {}/{} && ln -sf ../toolbox/shims/{} {}/{}",
            encoded, shims_dir, shim.name, shims_dir, shim.name, shim.name, bin_dir, shim.name
        );
        commands.push(shim_cmd);
    }

    commands
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

        let new_content = generate_system_packages(&["htop".to_string(), "vim".to_string()], false);
        editor.update_section(Section::SystemPackages, new_content);

        let rendered = editor.render();
        assert!(rendered.contains("htop"));
        assert!(rendered.contains("vim"));
    }

    #[test]
    fn test_generate_system_packages() {
        let packages = vec!["vim".to_string(), "htop".to_string(), "curl".to_string()];
        let lines = generate_system_packages(&packages, false);

        assert!(lines[0].contains("dnf install"));
        // Should be sorted alphabetically
        assert!(lines[1].contains("curl"));
        assert!(lines[2].contains("htop"));
        assert!(lines[3].contains("vim"));
    }

    #[test]
    fn cache_arg_name_sanitizes_repo_names() {
        assert_eq!(cache_arg_name("code"), "CACHE_EPOCH_CODE");
        assert_eq!(
            cache_arg_name("microsoft-edge"),
            "CACHE_EPOCH_MICROSOFT_EDGE"
        );
        assert_eq!(cache_arg_name("1password"), "CACHE_EPOCH_1PASSWORD");
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
        let lines = generate_system_packages(&packages, false);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "# No packages configured");
    }

    #[test]
    fn test_generate_system_packages_format() {
        // Verify exact output format including trailing backslashes
        let packages = vec!["pkg1".to_string(), "pkg2".to_string()];
        let lines = generate_system_packages(&packages, false);

        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "RUN dnf install -y \\");
        assert_eq!(lines[1], "    pkg1 \\");
        assert_eq!(lines[2], "    pkg2 \\");
        assert_eq!(lines[3], "    && dnf clean all");
    }

    #[test]
    fn test_generate_system_packages_with_external_rpms() {
        let packages = vec!["pkg1".to_string(), "pkg2".to_string()];
        let lines = generate_system_packages(&packages, true);

        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "RUN dnf install -y \\");
        assert_eq!(lines[1], "    /tmp/rpms/*.rpm \\");
        assert_eq!(lines[2], "    pkg1 \\");
        assert_eq!(lines[3], "    pkg2 \\");
        assert_eq!(lines[4], "    && dnf clean all");
    }

    #[test]
    fn test_generate_system_packages_external_rpms_only() {
        let packages: Vec<String> = vec![];
        let lines = generate_system_packages(&packages, true);

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "RUN dnf install -y \\");
        assert_eq!(lines[1], "    /tmp/rpms/*.rpm \\");
        assert_eq!(lines[2], "    && dnf clean all");
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
    fn test_generate_host_shim_commands_empty() {
        let shims: Vec<Shim> = vec![];
        let commands = generate_host_shim_commands(&shims);

        assert!(commands.is_empty());
    }

    #[test]
    fn test_generate_host_shim_commands_single() {
        let shims = vec![sample_shim("bootc")];
        let commands = generate_host_shim_commands(&shims);

        // Check structure
        assert!(commands[0].contains("mkdir -p"));
        assert!(commands[0].contains("/usr/etc/skel/.local/toolbox/shims"));
        assert!(commands[0].contains("/usr/etc/skel/.local/bin"));

        // Check shim creation uses base64 encoding
        let content = commands.join("\n");
        assert!(content.contains("base64 -d > /usr/etc/skel/.local/toolbox/shims/bootc"));
        assert!(content.contains("chmod 0755"));
        assert!(content.contains("ln -sf ../toolbox/shims/bootc /usr/etc/skel/.local/bin/bootc"));

        // Verify the base64 decodes correctly
        let expected_script = "#!/bin/bash\nexec flatpak-spawn --host bootc \"$@\"\n";
        let encoded = BASE64_STANDARD.encode(expected_script.as_bytes());
        assert!(content.contains(&encoded));
    }

    #[test]
    fn test_generate_host_shim_commands_multiple() {
        let shims = vec![
            sample_shim("systemctl"),
            sample_shim("bootc"),
            sample_shim("podman"),
        ];
        let commands = generate_host_shim_commands(&shims);
        let content = commands.join("\n");

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
    fn test_generate_host_shim_commands_with_host_override() {
        let shims = vec![sample_shim_with_host("docker", "podman")];
        let commands = generate_host_shim_commands(&shims);
        let content = commands.join("\n");

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
    #[test]
    fn test_generate_full_containerfile_contains_sections() {
        let input = ContainerfileGeneratorInput {
            external_repos: ExternalReposManifest::default(),
            upstreams: UpstreamManifest::default(),
            packages: Vec::new(),
            copr_repos: Vec::new(),
            system_config: SystemConfigManifest::default(),
            image_config: ImageConfigManifest {
                schema: None,
                modules: Vec::new(),
            },
            shims: Vec::new(),
            has_external_rpms: false,
        };

        let output = generate_full_containerfile(&input);

        assert!(output.contains(&section_header("Tools stage")));
        assert!(output.contains("FROM base AS image"));
        assert!(output.contains("# === SYSTEM_PACKAGES (managed by bkt) ==="));
        assert!(output.contains("# === RPM VERSION SNAPSHOT ==="));
        assert!(output.ends_with('\n'));
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
