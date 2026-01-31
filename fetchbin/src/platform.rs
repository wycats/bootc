#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
    Unknown(String),
}

impl Os {
    pub fn as_str(&self) -> &str {
        match self {
            Os::Linux => "linux",
            Os::MacOs => "macos",
            Os::Windows => "windows",
            Os::Unknown(value) => value.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
    Armv7,
    Unknown(String),
}

impl Arch {
    pub fn as_str(&self) -> &str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::Armv7 => "armv7",
            Arch::Unknown(value) => value.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    pub fn current() -> Self {
        let os = match std::env::consts::OS {
            "linux" => Os::Linux,
            "macos" => Os::MacOs,
            "windows" => Os::Windows,
            other => Os::Unknown(other.to_string()),
        };

        let arch = match std::env::consts::ARCH {
            "x86_64" => Arch::X86_64,
            "aarch64" => Arch::Aarch64,
            "arm" | "armv7" => Arch::Armv7,
            other => Arch::Unknown(other.to_string()),
        };

        Self { os, arch }
    }

    pub fn asset_patterns(&self) -> Vec<&'static str> {
        match (&self.os, &self.arch) {
            (Os::Linux, Arch::X86_64) => vec![
                "Linux_x86_64",
                "linux-x64",
                "linux-amd64",
                "x86_64-linux",
                "x86_64-unknown-linux-gnu",
            ],
            (Os::Linux, Arch::Aarch64) => vec![
                "Linux_arm64",
                "linux-arm64",
                "linux-aarch64",
                "aarch64-linux",
                "aarch64-unknown-linux-gnu",
            ],
            (Os::MacOs, Arch::X86_64) => vec![
                "*darwin*x64*",
                "*darwin*amd64*",
                "*macos*x64*",
                "*osx*x64*",
                "*x86_64*apple*darwin*",
            ],
            (Os::MacOs, Arch::Aarch64) => vec![
                "*darwin*arm64*",
                "*macos*arm64*",
                "*apple*silicon*",
                "*aarch64*apple*darwin*",
            ],
            (Os::Windows, Arch::X86_64) => vec![
                "*windows*x64*",
                "*win64*",
                "*win*x64*",
                "*x86_64*windows*",
                "*.exe",
            ],
            (Os::Windows, Arch::Aarch64) => vec!["*windows*arm64*", "*win*arm64*"],
            _ => Vec::new(),
        }
    }

    pub fn matches_asset(&self, asset_name: &str) -> bool {
        let asset_name = asset_name.to_lowercase();
        self.asset_patterns().iter().any(|pattern| {
            let pattern = pattern.to_lowercase();
            if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
                if let Ok(glob) = glob::Pattern::new(&pattern) {
                    glob.matches(&asset_name)
                } else {
                    false
                }
            } else {
                asset_name.contains(&pattern)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_platform_is_valid() {
        let platform = Platform::current();
        assert!(!platform.os.as_str().is_empty());
        assert!(!platform.arch.as_str().is_empty());
    }

    #[test]
    fn test_asset_pattern_matching_linux_x86_64() {
        let platform = Platform {
            os: Os::Linux,
            arch: Arch::X86_64,
        };

        assert!(platform.matches_asset("tool_Linux_x86_64.tar.gz"));
        assert!(platform.matches_asset("tool-linux-x64.zip"));
        assert!(platform.matches_asset("tool-linux-amd64"));
        assert!(platform.matches_asset("tool-x86_64-linux"));
        assert!(platform.matches_asset("tool-x86_64-unknown-linux-gnu"));
        assert!(!platform.matches_asset("tool-darwin-x86_64"));
    }

    #[test]
    fn test_asset_pattern_matching_linux_aarch64() {
        let platform = Platform {
            os: Os::Linux,
            arch: Arch::Aarch64,
        };

        assert!(platform.matches_asset("tool_Linux_arm64.tar.gz"));
        assert!(platform.matches_asset("tool-linux-arm64.zip"));
        assert!(platform.matches_asset("tool-linux-aarch64"));
        assert!(platform.matches_asset("tool-aarch64-linux"));
        assert!(platform.matches_asset("tool-aarch64-unknown-linux-gnu"));
        assert!(!platform.matches_asset("tool-linux-x86_64"));
    }

    #[test]
    fn test_asset_pattern_matching() {
        let platform = Platform {
            os: Os::Linux,
            arch: Arch::X86_64,
        };

        assert!(platform.matches_asset("tool-linux-x64.tar.gz"));
        assert!(!platform.matches_asset("tool-darwin-x86_64.tar.gz"));
    }

    #[test]
    fn test_asset_pattern_matching_macos_x86_64() {
        let platform = Platform {
            os: Os::MacOs,
            arch: Arch::X86_64,
        };

        assert!(platform.matches_asset("tool-darwin-x64.tar.gz"));
        assert!(platform.matches_asset("tool-macos-x64.zip"));
        assert!(platform.matches_asset("tool-x86_64-apple-darwin"));
        assert!(!platform.matches_asset("tool-linux-x64.tar.gz"));
    }

    #[test]
    fn test_asset_pattern_matching_macos_aarch64() {
        let platform = Platform {
            os: Os::MacOs,
            arch: Arch::Aarch64,
        };

        assert!(platform.matches_asset("tool-darwin-arm64.tar.gz"));
        assert!(platform.matches_asset("tool-macos-arm64.zip"));
        assert!(platform.matches_asset("tool-aarch64-apple-darwin"));
        assert!(!platform.matches_asset("tool-darwin-x64.tar.gz"));
    }

    #[test]
    fn test_asset_pattern_matching_windows_x86_64() {
        let platform = Platform {
            os: Os::Windows,
            arch: Arch::X86_64,
        };

        assert!(platform.matches_asset("tool-windows-x64.exe"));
        assert!(platform.matches_asset("tool-win64.zip"));
        assert!(platform.matches_asset("tool-x86_64-windows"));
        assert!(!platform.matches_asset("tool-linux-x64.tar.gz"));
    }

    #[test]
    fn test_asset_pattern_matching_windows_aarch64() {
        let platform = Platform {
            os: Os::Windows,
            arch: Arch::Aarch64,
        };

        assert!(platform.matches_asset("tool-windows-arm64.exe"));
        assert!(platform.matches_asset("tool-win-arm64.zip"));
        assert!(!platform.matches_asset("tool-windows-x64.exe"));
    }
}
