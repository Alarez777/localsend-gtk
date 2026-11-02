//! Best-effort package-manager hints for optional integration dependencies.
//!
//! The application never installs system packages itself. It only shows a
//! command the user can review and run with their own privileges.
use crate::file_managers::Requirement;
use std::fs;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageManager {
    Apt,
    Dnf,
    Pacman,
    Zypper,
    Unknown,
}

impl PackageManager {
    pub fn detect() -> Self {
        let contents = fs::read_to_string("/etc/os-release")
            .or_else(|_| fs::read_to_string("/usr/lib/os-release"))
            .unwrap_or_default();
        let mut ids = Vec::new();
        for line in contents.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key != "ID" && key != "ID_LIKE" {
                continue;
            }
            ids.extend(
                value
                    .trim_matches(['"', '\''])
                    .split_whitespace()
                    .map(str::to_ascii_lowercase),
            );
        }
        if ids.iter().any(|id| {
            matches!(
                id.as_str(),
                "debian"
                    | "ubuntu"
                    | "linuxmint"
                    | "pop"
                    | "elementary"
                    | "zorin"
                    | "kali"
                    | "neon"
            )
        }) {
            Self::Apt
        } else if ids.iter().any(|id| {
            matches!(
                id.as_str(),
                "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "nobara"
            )
        }) {
            Self::Dnf
        } else if ids
            .iter()
            .any(|id| matches!(id.as_str(), "arch" | "manjaro" | "endeavouros" | "garuda"))
        {
            Self::Pacman
        } else if ids.iter().any(|id| {
            matches!(
                id.as_str(),
                "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "suse" | "sled"
            )
        }) {
            Self::Zypper
        } else {
            Self::Unknown
        }
    }

    pub fn install_command(self, missing: &[&Requirement]) -> Option<String> {
        let mut packages = Vec::new();
        for requirement in missing {
            for package in self.packages_for(requirement.package()) {
                if !packages.contains(&package) {
                    packages.push(package);
                }
            }
        }
        if packages.is_empty() {
            return None;
        }
        let packages = packages.join(" ");
        match self {
            Self::Apt => Some(format!("sudo apt install {packages}")),
            Self::Dnf => Some(format!("sudo dnf install {packages}")),
            Self::Pacman => Some(format!("sudo pacman -S {packages}")),
            Self::Zypper => Some(format!("sudo zypper install {packages}")),
            Self::Unknown => None,
        }
    }

    fn packages_for(self, package: &str) -> Vec<&str> {
        match (self, package) {
            (Self::Apt, "nautilus-python") => vec!["python3-nautilus", "python3-gi"],
            (Self::Dnf, "nautilus-python") => vec!["nautilus-python", "python3-gobject"],
            (Self::Pacman, "nautilus-python") => vec!["python-nautilus", "python-gobject"],
            (Self::Zypper, "nautilus-python") => vec!["python3-nautilus", "python3-gobject"],
            (_, "dolphin") => vec!["dolphin"],
            (_, "nautilus") => vec!["nautilus"],
            (_, "bash") => vec!["bash"],
            (_, other) => vec![other],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirement(package: &'static str) -> Requirement {
        Requirement::Command {
            command: package,
            package,
        }
    }

    #[test]
    fn bridge_package_names_are_distro_specific() {
        let missing = [&Requirement::NautilusPython];
        assert_eq!(
            PackageManager::Apt.install_command(&missing).as_deref(),
            Some("sudo apt install python3-nautilus python3-gi")
        );
        assert_eq!(
            PackageManager::Dnf.install_command(&missing).as_deref(),
            Some("sudo dnf install nautilus-python python3-gobject")
        );
        assert_eq!(
            PackageManager::Pacman.install_command(&missing).as_deref(),
            Some("sudo pacman -S python-nautilus python-gobject")
        );
        assert_eq!(PackageManager::Unknown.install_command(&missing), None);
    }

    #[test]
    fn commands_keep_only_unique_packages() {
        let nautilus = requirement("nautilus");
        let bash = requirement("bash");
        let missing = [&nautilus, &nautilus, &bash];
        assert_eq!(
            PackageManager::Apt.install_command(&missing).as_deref(),
            Some("sudo apt install nautilus bash")
        );
    }
}
