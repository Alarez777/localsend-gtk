//! Optional per-user file manager integrations.
//!
//! These integrations are never installed by packages or at startup. The
//! settings view calls this module only when the user explicitly installs or
//! removes one. All files live below `XDG_DATA_HOME`; system-wide copies are
//! reported but never modified.
use crate::desktop::desktop_exec_argument;
use std::{
    env, fmt, fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const APP_ID: &str = "org.localsend.localsend_gtk";
const OWNER_KEY: &str = "X-LocalSend-GTK-Owner";
const OWNER_COMMENT: &str = "# X-LocalSend-GTK-Owner: org.localsend.localsend_gtk";

const DOLPHIN_TEMPLATE: &str = include_str!("../assets/integrations/localsend-dolphin.desktop");
const NAUTILUS_TEMPLATE: &str = include_str!("../assets/integrations/nautilus-send-with-localsend");
const DOLPHIN_EXEC_MARKER: &str = "@LOCALSEND_GTK_EXEC@";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Requirement {
    pub command: &'static str,
    pub package: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileManagerIntegration {
    Dolphin,
    Nautilus,
}

impl FileManagerIntegration {
    pub const ALL: [Self; 2] = [Self::Dolphin, Self::Nautilus];

    pub fn id(self) -> &'static str {
        match self {
            Self::Dolphin => "dolphin",
            Self::Nautilus => "nautilus",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Dolphin => "Dolphin",
            Self::Nautilus => "Nautilus (GNOME Files)",
        }
    }

    pub fn requirements(self) -> &'static [Requirement] {
        static DOLPHIN: [Requirement; 1] = [Requirement {
            command: "dolphin",
            package: "dolphin",
        }];
        static NAUTILUS: [Requirement; 2] = [
            Requirement {
                command: "nautilus",
                package: "nautilus",
            },
            Requirement {
                command: "bash",
                package: "bash",
            },
        ];
        match self {
            Self::Dolphin => &DOLPHIN,
            Self::Nautilus => &NAUTILUS,
        }
    }

    fn relative_path(self) -> &'static str {
        match self {
            Self::Dolphin => "kio/servicemenus/localsend-dolphin.desktop",
            Self::Nautilus => "nautilus/scripts/Send with LocalSend",
        }
    }

    pub fn target(self, environment: &Environment) -> PathBuf {
        environment.data_home.join(self.relative_path())
    }

    fn contents(self, executable: &Path) -> io::Result<String> {
        match self {
            Self::Dolphin => {
                let exec = desktop_exec_argument(executable)?;
                Ok(DOLPHIN_TEMPLATE.replace(DOLPHIN_EXEC_MARKER, &exec))
            }
            Self::Nautilus => {
                let quoted = shell_quote(executable)?;
                let assignment = format!("localsend_gtk_binary={quoted}");
                Ok(NAUTILUS_TEMPLATE.replacen("localsend_gtk_binary=", &assignment, 1))
            }
        }
    }

    fn owns(self, path: &Path) -> io::Result<bool> {
        let contents = fs::read_to_string(path)?;
        match self {
            Self::Dolphin => {
                let keyfile = glib::KeyFile::new();
                if keyfile
                    .load_from_data(&contents, glib::KeyFileFlags::NONE)
                    .is_err()
                {
                    return Ok(false);
                }
                Ok(keyfile.string("Desktop Entry", OWNER_KEY).ok().as_deref() == Some(APP_ID))
            }
            Self::Nautilus => Ok(contents.lines().any(|line| line.trim() == OWNER_COMMENT)),
        }
    }

    fn system_installed(self, environment: &Environment) -> bool {
        match self {
            // Nautilus only reads scripts from the user's data directory.
            Self::Nautilus => false,
            Self::Dolphin => environment
                .data_dirs
                .iter()
                .any(|directory| directory.join(self.relative_path()).is_file()),
        }
    }

    fn legacy_link_target(self, target: &Path) -> bool {
        match self {
            Self::Dolphin => target.ends_with(
                Path::new("kio")
                    .join("servicemenus")
                    .join("localsend-dolphin.desktop"),
            ),
            Self::Nautilus => {
                target.ends_with(Path::new("nautilus-scripts").join("Send with LocalSend"))
            }
        }
    }

    /// Older tar installers wrote recognizable files without an owner marker.
    /// Only these exact LocalSend payloads are replaceable; any other unowned
    /// file remains a conflict.
    fn legacy_file(self, path: &Path) -> io::Result<bool> {
        let contents = fs::read_to_string(path)?;
        match self {
            Self::Dolphin => {
                let keyfile = glib::KeyFile::new();
                if keyfile
                    .load_from_data(&contents, glib::KeyFileFlags::NONE)
                    .is_err()
                {
                    return Ok(false);
                }
                Ok(keyfile
                    .string("Desktop Entry", "Actions")
                    .is_ok_and(|actions| actions.contains("SendWithLocalSend"))
                    && keyfile
                        .string("Desktop Action SendWithLocalSend", "Exec")
                        .is_ok_and(|exec| exec.contains("localsend-gtk"))
                    && keyfile
                        .string("Desktop Entry", "Icon")
                        .is_ok_and(|icon| icon.contains("localsend-gtk")))
            }
            Self::Nautilus => Ok(contents.contains("GNOME Nautilus")
                && contents.contains("localsend_gtk_binary=")
                && contents.contains("localsend-gtk")),
        }
    }

    fn target_kind(self, path: &Path, environment: &Environment) -> io::Result<TargetKind> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let target = fs::read_link(path)?;
                if self.legacy_link_target(&target) {
                    Ok(TargetKind::Legacy)
                } else {
                    Ok(TargetKind::Conflict)
                }
            }
            Ok(metadata) if metadata.file_type().is_file() => {
                if self.owns(path)? {
                    Ok(TargetKind::Owned)
                } else if self.legacy_file(path)? {
                    Ok(TargetKind::Legacy)
                } else {
                    Ok(TargetKind::Conflict)
                }
            }
            Ok(_) => Ok(TargetKind::Conflict),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if self.system_installed(environment) {
                    Ok(TargetKind::SystemManaged)
                } else {
                    Ok(TargetKind::Missing)
                }
            }
            Err(error) => Err(error),
        }
    }

    pub fn inspect(self, environment: &Environment) -> Status {
        let missing = self
            .requirements()
            .iter()
            .filter(|requirement| !environment.command_exists(requirement.command))
            .collect();
        let path = self.target(environment);
        match self.target_kind(&path, environment) {
            Ok(TargetKind::Missing) => Status {
                state: InstallState::NotInstalled,
                missing,
                error: None,
            },
            Ok(TargetKind::Owned) => Status {
                state: InstallState::Installed,
                missing,
                error: None,
            },
            Ok(TargetKind::SystemManaged) => Status {
                state: InstallState::SystemManaged,
                missing,
                error: None,
            },
            Ok(TargetKind::Legacy) => Status {
                state: InstallState::Legacy,
                missing,
                error: None,
            },
            Ok(TargetKind::Conflict) => Status {
                state: InstallState::Conflict,
                missing,
                error: None,
            },
            Err(error) => Status {
                state: InstallState::Unknown,
                missing,
                error: Some(error.to_string()),
            },
        }
    }

    pub fn install(self, environment: &Environment) -> Result<(), IntegrationError> {
        if environment.flatpak {
            return Err(IntegrationError::Unsupported);
        }
        let executable = env::current_exe()?;
        let path = self.target(environment);
        match self.target_kind(&path, environment)? {
            TargetKind::Owned | TargetKind::Missing => {}
            TargetKind::Legacy => {
                // A user-created symlink to the old package file can be replaced
                // by this action; the linked system file remains untouched.
                fs::remove_file(&path)?;
            }
            TargetKind::Conflict => return Err(IntegrationError::Conflict),
            TargetKind::SystemManaged => return Err(IntegrationError::SystemManaged),
        }
        let contents = self.contents(&executable)?;
        write_executable(&path, contents.as_bytes())?;
        if self == Self::Dolphin {
            refresh_kde_cache(environment);
        }
        Ok(())
    }

    pub fn remove(self, environment: &Environment) -> Result<(), IntegrationError> {
        if environment.flatpak {
            return Err(IntegrationError::Unsupported);
        }
        let path = self.target(environment);
        match self.target_kind(&path, environment)? {
            TargetKind::Owned | TargetKind::Legacy => fs::remove_file(path)?,
            TargetKind::Missing => return Ok(()),
            TargetKind::Conflict => return Err(IntegrationError::Conflict),
            TargetKind::SystemManaged => return Err(IntegrationError::SystemManaged),
        }
        if self == Self::Dolphin {
            refresh_kde_cache(environment);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetKind {
    Missing,
    Owned,
    SystemManaged,
    Legacy,
    Conflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallState {
    NotInstalled,
    Installed,
    SystemManaged,
    Legacy,
    Conflict,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub state: InstallState,
    pub missing: Vec<&'static Requirement>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Environment {
    data_home: PathBuf,
    data_dirs: Vec<PathBuf>,
    search_path: Vec<PathBuf>,
    flatpak: bool,
}

impl Environment {
    pub fn current() -> io::Result<Self> {
        let data_home = xdg_data_home()?;
        let data_dirs = env::var_os("XDG_DATA_DIRS")
            .map(|value| {
                env::split_paths(&value)
                    .filter(|path| path.is_absolute())
                    .collect()
            })
            .filter(|paths: &Vec<_>| !paths.is_empty())
            .unwrap_or_else(|| {
                vec![
                    PathBuf::from("/usr/local/share"),
                    PathBuf::from("/usr/share"),
                ]
            });
        let search_path = env::var_os("PATH")
            .map(|value| env::split_paths(&value).collect())
            .unwrap_or_default();
        Ok(Self {
            data_home,
            data_dirs,
            search_path,
            flatpak: Path::new("/.flatpak-info").is_file(),
        })
    }

    pub fn is_flatpak(&self) -> bool {
        self.flatpak
    }

    fn command_exists(&self, command: &str) -> bool {
        self.search_path.iter().any(|directory| {
            let path = directory.join(command);
            path.is_file() && is_executable(&path)
        })
    }

    /// Deterministic environment for the headless GTK fixture. It never reads
    /// or writes the real user's data directories.
    pub fn fixture() -> Self {
        Self {
            data_home: PathBuf::from("/nonexistent/localsend-gtk-test"),
            data_dirs: Vec::new(),
            search_path: Vec::new(),
            flatpak: false,
        }
    }

    #[cfg(test)]
    fn for_test(root: &Path, commands: &[&str], flatpak: bool) -> Self {
        let bin = root.join("bin");
        for command in commands {
            let path = bin.join(command);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        Self {
            data_home: root.join("data"),
            data_dirs: vec![root.join("system")],
            search_path: vec![bin],
            flatpak,
        }
    }
}

fn xdg_data_home() -> io::Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Ok(path);
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_owned()))
        .ok_or_else(|| io::Error::other("Could not locate the user data directory"))?;
    Ok(home.join(".local/share"))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn shell_quote(executable: &Path) -> io::Result<String> {
    let text = executable.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "The integration requires a UTF-8 executable path",
        )
    })?;
    if !executable.is_absolute() || text.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The integration requires an absolute executable path without control characters.",
        ));
    }
    Ok(format!("'{}'", text.replace('\'', "'\\''")))
}

fn write_executable(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing integration directory"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))?;
        }
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn refresh_kde_cache(environment: &Environment) {
    if environment.flatpak {
        return;
    }
    let helper = ["kbuildsycoca6", "kbuildsycoca5"]
        .into_iter()
        .find(|command| environment.command_exists(command));
    let Some(helper) = helper else {
        return;
    };
    // The menu must not block the GTK thread; a missing or slow cache helper is
    // harmless because KDE also discovers service menus on its own.
    std::thread::spawn(move || {
        let _ = Command::new(helper)
            .arg("--noincremental")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    });
}

#[derive(Debug)]
pub enum IntegrationError {
    Unsupported,
    Conflict,
    SystemManaged,
    Io(io::Error),
}

impl fmt::Display for IntegrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => {
                write!(
                    formatter,
                    "File manager integrations are not available in Flatpak."
                )
            }
            Self::Conflict => write!(
                formatter,
                "A file not created by LocalSend already uses this name."
            ),
            Self::SystemManaged => write!(
                formatter,
                "This integration is managed by your system package."
            ),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for IntegrationError {}

impl From<io::Error> for IntegrationError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn target(integration: FileManagerIntegration, environment: &Environment) -> PathBuf {
        integration.target(environment)
    }

    #[test]
    fn installs_and_removes_user_integrations_with_executable_permissions() {
        let root = tempfile::tempdir().unwrap();
        let environment =
            Environment::for_test(root.path(), &["dolphin", "nautilus", "bash"], false);

        for integration in FileManagerIntegration::ALL {
            let status = integration.inspect(&environment);
            assert_eq!(status.state, InstallState::NotInstalled);
            assert!(status.missing.is_empty());

            integration.install(&environment).unwrap();
            let path = target(integration, &environment);
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert!(metadata.file_type().is_file());
            assert_eq!(metadata.permissions().mode() & 0o777, 0o755);
            let contents = fs::read_to_string(&path).unwrap();
            assert!(contents.contains(APP_ID));
            assert_eq!(
                integration.inspect(&environment).state,
                InstallState::Installed
            );

            integration.remove(&environment).unwrap();
            assert!(!path.exists());
            assert_eq!(
                integration.inspect(&environment).state,
                InstallState::NotInstalled
            );
        }
    }

    #[test]
    fn dolphin_menu_is_valid_and_uses_the_current_executable() {
        let root = tempfile::tempdir().unwrap();
        let environment = Environment::for_test(root.path(), &["dolphin"], false);
        let integration = FileManagerIntegration::Dolphin;
        integration.install(&environment).unwrap();
        let contents = fs::read_to_string(target(integration, &environment)).unwrap();
        let keyfile = glib::KeyFile::new();
        keyfile
            .load_from_data(&contents, glib::KeyFileFlags::NONE)
            .unwrap();
        assert_eq!(keyfile.string("Desktop Entry", OWNER_KEY).unwrap(), APP_ID);
        let exec = keyfile
            .string("Desktop Action SendWithLocalSend", "Exec")
            .unwrap();
        assert!(exec.ends_with(" %U"));
        assert!(exec.contains(&env::current_exe().unwrap().to_string_lossy().into_owned()));
    }

    #[test]
    fn nautilus_script_is_valid_shell_and_keeps_the_owner_marker() {
        let root = tempfile::tempdir().unwrap();
        let environment = Environment::for_test(root.path(), &["nautilus", "bash"], false);
        let integration = FileManagerIntegration::Nautilus;
        integration.install(&environment).unwrap();
        let path = target(integration, &environment);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains(OWNER_COMMENT));
        assert!(contents.contains("localsend_gtk_binary='"));
        assert!(Command::new("bash")
            .args(["-n", path.to_str().unwrap()])
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn unusual_executable_paths_are_escaped_for_desktop_and_shell_files() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("Local Send '$`\\\";%f青");
        let dolphin = FileManagerIntegration::Dolphin
            .contents(&executable)
            .unwrap();
        assert!(dolphin.contains("/usr/bin/env -- "));
        assert!(dolphin.contains("%%f"));
        assert!(dolphin.contains(" %U"));

        let script = FileManagerIntegration::Nautilus
            .contents(&executable)
            .unwrap();
        let assignment = script
            .lines()
            .find(|line| line.starts_with("localsend_gtk_binary="))
            .unwrap();
        assert_eq!(
            assignment,
            format!(
                "localsend_gtk_binary='{}'",
                executable.to_string_lossy().replace('\'', "'\\''")
            )
        );
    }

    #[test]
    fn unowned_files_are_reported_and_never_replaced_or_deleted() {
        let root = tempfile::tempdir().unwrap();
        let environment =
            Environment::for_test(root.path(), &["dolphin", "nautilus", "bash"], false);
        for integration in FileManagerIntegration::ALL {
            let path = target(integration, &environment);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "user content").unwrap();
            assert_eq!(
                integration.inspect(&environment).state,
                InstallState::Conflict
            );
            assert!(matches!(
                integration.install(&environment),
                Err(IntegrationError::Conflict)
            ));
            assert!(matches!(
                integration.remove(&environment),
                Err(IntegrationError::Conflict)
            ));
            assert_eq!(fs::read_to_string(&path).unwrap(), "user content");
        }
    }

    #[test]
    fn legacy_user_files_without_markers_can_be_replaced_and_removed() {
        let root = tempfile::tempdir().unwrap();
        let environment =
            Environment::for_test(root.path(), &["dolphin", "nautilus", "bash"], false);
        for (integration, legacy) in [
            (
                FileManagerIntegration::Dolphin,
                "[Desktop Entry]\nType=Service\nActions=SendWithLocalSend;\nIcon=/home/user/.local/share/icons/hicolor/512x512/apps/localsend-gtk.png\n\n[Desktop Action SendWithLocalSend]\nName=Send with LocalSend\nExec=/home/user/.local/bin/localsend-gtk %U\n",
            ),
            (
                FileManagerIntegration::Nautilus,
                "#!/usr/bin/env bash\n# GNOME Nautilus / Files script to send selected files via LocalSend GTK.\nlocalsend_gtk_binary=\nexec localsend-gtk -- \"$@\"\n",
            ),
        ] {
            let path = target(integration, &environment);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, legacy).unwrap();
            assert_eq!(integration.inspect(&environment).state, InstallState::Legacy);

            integration.install(&environment).unwrap();
            assert_eq!(
                integration.inspect(&environment).state,
                InstallState::Installed
            );

            fs::write(&path, legacy).unwrap();
            integration.remove(&environment).unwrap();
            assert!(!path.exists());
        }
    }

    #[test]
    fn legacy_system_symlinks_can_be_replaced_without_touching_the_target() {
        let root = tempfile::tempdir().unwrap();
        let environment = Environment::for_test(root.path(), &["nautilus", "bash"], false);
        let integration = FileManagerIntegration::Nautilus;
        let system_file = environment
            .data_dirs
            .first()
            .unwrap()
            .join("nautilus-scripts/Send with LocalSend");
        fs::create_dir_all(system_file.parent().unwrap()).unwrap();
        fs::write(&system_file, "package copy").unwrap();
        let path = target(integration, &environment);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&system_file, &path).unwrap();

        assert_eq!(
            integration.inspect(&environment).state,
            InstallState::Legacy
        );
        integration.install(&environment).unwrap();
        assert!(fs::symlink_metadata(&path).unwrap().file_type().is_file());
        assert_eq!(fs::read_to_string(&system_file).unwrap(), "package copy");

        integration.remove(&environment).unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read_to_string(system_file).unwrap(), "package copy");
    }

    #[test]
    fn unrelated_symlinks_are_conflicts() {
        let root = tempfile::tempdir().unwrap();
        let environment = Environment::for_test(root.path(), &["nautilus", "bash"], false);
        let integration = FileManagerIntegration::Nautilus;
        let other = root.path().join("other");
        fs::write(&other, "foreign").unwrap();
        let path = target(integration, &environment);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&other, &path).unwrap();
        assert_eq!(
            integration.inspect(&environment).state,
            InstallState::Conflict
        );
        assert!(matches!(
            integration.remove(&environment),
            Err(IntegrationError::Conflict)
        ));
        assert_eq!(fs::read_to_string(other).unwrap(), "foreign");
    }

    #[test]
    fn system_package_integrations_are_read_only() {
        let root = tempfile::tempdir().unwrap();
        let environment = Environment::for_test(root.path(), &["dolphin"], false);
        let integration = FileManagerIntegration::Dolphin;
        let system_file = environment
            .data_dirs
            .first()
            .unwrap()
            .join("kio/servicemenus/localsend-dolphin.desktop");
        fs::create_dir_all(system_file.parent().unwrap()).unwrap();
        fs::write(&system_file, "package copy").unwrap();
        assert_eq!(
            integration.inspect(&environment).state,
            InstallState::SystemManaged
        );
        assert!(matches!(
            integration.remove(&environment),
            Err(IntegrationError::SystemManaged)
        ));
        assert_eq!(fs::read_to_string(system_file).unwrap(), "package copy");
    }

    #[test]
    fn missing_file_manager_and_flatpak_are_reported() {
        let root = tempfile::tempdir().unwrap();
        let environment = Environment::for_test(root.path(), &[], false);
        let status = FileManagerIntegration::Nautilus.inspect(&environment);
        assert_eq!(
            status
                .missing
                .iter()
                .map(|requirement| requirement.package)
                .collect::<Vec<_>>(),
            ["nautilus", "bash"]
        );

        let flatpak = Environment::for_test(root.path(), &["nautilus", "bash"], true);
        assert!(matches!(
            FileManagerIntegration::Nautilus.install(&flatpak),
            Err(IntegrationError::Unsupported)
        ));
    }

    #[test]
    fn data_home_follows_xdg_layout() {
        let environment = Environment::for_test(Path::new("/tmp/root"), &[], false);
        assert_eq!(
            FileManagerIntegration::Dolphin.target(&environment),
            Path::new("/tmp/root/data/kio/servicemenus/localsend-dolphin.desktop")
        );
        assert_eq!(
            FileManagerIntegration::Nautilus.target(&environment),
            Path::new("/tmp/root/data/nautilus/scripts/Send with LocalSend")
        );
    }
}
