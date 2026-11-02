//! Optional per-user file manager integrations.
//!
//! These integrations are never installed by packages or at startup. The
//! settings view calls this module only when the user explicitly installs or
//! removes one. All files live below `XDG_DATA_HOME`; system-wide copies are
//! reported but never modified.
use crate::{
    desktop::desktop_exec_argument,
    i18n::{self, Locale},
};
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
const NAUTILUS_MENU_TEMPLATE: &str = include_str!("../assets/integrations/nautilus-menu.py");
const NEMO_TEMPLATE: &str = include_str!("../assets/integrations/nemo-action.nemo_action");
const DOLPHIN_EXEC_MARKER: &str = "@LOCALSEND_GTK_EXEC@";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requirement {
    Command {
        command: &'static str,
        package: &'static str,
    },
    NautilusPython,
}

impl Requirement {
    pub fn package(self) -> &'static str {
        match self {
            Self::Command { package, .. } => package,
            Self::NautilusPython => "nautilus-python",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileManagerIntegration {
    Dolphin,
    NautilusScript,
    NautilusMenu,
    Nemo,
}

impl FileManagerIntegration {
    pub const ALL: [Self; 4] = [
        Self::Dolphin,
        Self::NautilusScript,
        Self::NautilusMenu,
        Self::Nemo,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Dolphin => "dolphin",
            Self::NautilusScript => "nautilus-script",
            Self::NautilusMenu => "nautilus-menu",
            Self::Nemo => "nemo",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Dolphin => "Dolphin",
            Self::NautilusScript => "Nautilus (Scripts)",
            Self::NautilusMenu => "Nautilus (direct menu · requires nautilus-python)",
            Self::Nemo => "Nemo",
        }
    }

    pub fn requirements(self) -> &'static [Requirement] {
        static DOLPHIN: [Requirement; 1] = [Requirement::Command {
            command: "dolphin",
            package: "dolphin",
        }];
        static NAUTILUS_SCRIPT: [Requirement; 2] = [
            Requirement::Command {
                command: "nautilus",
                package: "nautilus",
            },
            Requirement::Command {
                command: "bash",
                package: "bash",
            },
        ];
        static NAUTILUS_MENU: [Requirement; 2] = [
            Requirement::Command {
                command: "nautilus",
                package: "nautilus",
            },
            Requirement::NautilusPython,
        ];
        static NEMO: [Requirement; 1] = [Requirement::Command {
            command: "nemo",
            package: "nemo",
        }];
        match self {
            Self::Dolphin => &DOLPHIN,
            Self::NautilusScript => &NAUTILUS_SCRIPT,
            Self::NautilusMenu => &NAUTILUS_MENU,
            Self::Nemo => &NEMO,
        }
    }

    fn relative_path(self, locale: Locale) -> PathBuf {
        match self {
            Self::Dolphin => PathBuf::from("kio/servicemenus/localsend-dolphin.desktop"),
            Self::NautilusScript => {
                Path::new("nautilus/scripts").join(i18n::tr_for(locale, "Send with LocalSend"))
            }
            Self::NautilusMenu => PathBuf::from("nautilus-python/extensions/localsend-gtk.py"),
            Self::Nemo => PathBuf::from("nemo/actions/localsend-gtk.nemo_action"),
        }
    }

    pub fn target(self, environment: &Environment) -> PathBuf {
        environment
            .data_home
            .join(self.relative_path(environment.locale))
    }

    fn contents(self, executable: &Path, locale: Locale) -> io::Result<String> {
        match self {
            Self::Dolphin => {
                let exec = desktop_exec_argument(executable)?;
                let name = i18n::tr_for(locale, "Send with LocalSend");
                Ok(DOLPHIN_TEMPLATE
                    .replacen("Name=Send with LocalSend", &format!("Name={name}"), 1)
                    .replace(DOLPHIN_EXEC_MARKER, &exec))
            }
            Self::NautilusScript => {
                let quoted = shell_quote(executable)?;
                let assignment = format!("localsend_gtk_binary={quoted}");
                Ok(NAUTILUS_TEMPLATE
                    .replace(
                        "Only valid local files and folders can be sent.",
                        &i18n::tr_for(locale, "Only valid local files and folders can be sent."),
                    )
                    .replace(
                        "Select one or more local files or folders to send.",
                        &i18n::tr_for(locale, "Select one or more local files or folders to send."),
                    )
                    .replace(
                        "Could not find localsend-gtk binary on PATH.",
                        &i18n::tr_for(locale, "Could not find localsend-gtk binary on PATH."),
                    )
                    .replacen("localsend_gtk_binary=", &assignment, 1))
            }
            Self::NautilusMenu => {
                let binary = python_string(executable)?;
                Ok(NAUTILUS_MENU_TEMPLATE
                    .replace("@LOCALSEND_GTK_BINARY@", &binary)
                    .replace(
                        "@LOCALSEND_GTK_ACTION@",
                        &python_text(i18n::tr_for(locale, "Send with LocalSend")),
                    )
                    .replace(
                        "@LOCALSEND_GTK_NOT_FOUND@",
                        &python_text(i18n::tr_for(
                            locale,
                            "Could not find localsend-gtk binary on PATH.",
                        )),
                    ))
            }
            Self::Nemo => {
                let exec = nemo_exec_argument(executable)?;
                let icon = if crate::desktop_integration::appimage_path().is_some() {
                    APP_ID
                } else {
                    "localsend-gtk"
                };
                Ok(NEMO_TEMPLATE
                    .replace(
                        "@LOCALSEND_GTK_NAMES@",
                        &localized_action_lines("Name", locale),
                    )
                    .replace(
                        "@LOCALSEND_GTK_COMMENTS@",
                        &localized_action_lines("Comment", locale),
                    )
                    .replace("@LOCALSEND_GTK_EXEC@", &exec)
                    .replace("@LOCALSEND_GTK_ICON@", icon))
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
            Self::NautilusScript | Self::NautilusMenu => {
                Ok(contents.lines().any(|line| line.trim() == OWNER_COMMENT))
            }
            Self::Nemo => {
                let keyfile = glib::KeyFile::new();
                if keyfile
                    .load_from_data(&contents, glib::KeyFileFlags::NONE)
                    .is_err()
                {
                    return Ok(false);
                }
                Ok(keyfile.string("Nemo Action", OWNER_KEY).ok().as_deref() == Some(APP_ID))
            }
        }
    }

    fn system_installed(self, environment: &Environment) -> bool {
        match self {
            // Nautilus only reads scripts from the user's data directory.
            Self::NautilusScript | Self::NautilusMenu => false,
            Self::Dolphin | Self::Nemo => environment.data_dirs.iter().any(|directory| {
                directory
                    .join(self.relative_path(environment.locale))
                    .is_file()
            }),
        }
    }

    fn legacy_link_target(self, target: &Path) -> bool {
        match self {
            Self::Dolphin => target.ends_with(
                Path::new("kio")
                    .join("servicemenus")
                    .join("localsend-dolphin.desktop"),
            ),
            Self::NautilusScript => {
                target.ends_with(Path::new("nautilus-scripts").join("Send with LocalSend"))
            }
            Self::NautilusMenu | Self::Nemo => false,
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
            Self::NautilusScript => Ok(contents.contains("GNOME Nautilus")
                && contents.contains("localsend_gtk_binary=")
                && contents.contains("localsend-gtk")),
            Self::NautilusMenu | Self::Nemo => Ok(false),
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

    fn installed_nautilus_scripts(
        self,
        environment: &Environment,
    ) -> io::Result<Vec<InstalledScript>> {
        if self != Self::NautilusScript {
            return Ok(Vec::new());
        }
        let directory = environment.data_home.join("nautilus/scripts");
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut result = Vec::new();
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                let Ok(target) = fs::read_link(&path) else {
                    continue;
                };
                if self.legacy_link_target(&target) {
                    result.push(InstalledScript { path, owned: false });
                }
            } else if metadata.file_type().is_file() {
                if self.owns(&path).unwrap_or(false) {
                    result.push(InstalledScript { path, owned: true });
                } else if self.legacy_file(&path).unwrap_or(false) {
                    result.push(InstalledScript { path, owned: false });
                }
            }
        }
        Ok(result)
    }

    pub fn inspect(self, environment: &Environment) -> Status {
        let missing = self
            .requirements()
            .iter()
            .filter(|requirement| !environment.requirement_met(**requirement))
            .collect();
        let path = self.target(environment);
        let state = match self.target_kind(&path, environment) {
            Ok(TargetKind::Missing) if self == Self::NautilusScript => {
                match self.installed_nautilus_scripts(environment) {
                    Ok(files) if files.iter().any(|file| file.owned) => InstallState::Installed,
                    Ok(files) if !files.is_empty() => InstallState::Legacy,
                    Ok(_) => InstallState::NotInstalled,
                    Err(error) => {
                        return Status {
                            state: InstallState::Unknown,
                            missing,
                            error: Some(error.to_string()),
                        }
                    }
                }
            }
            Ok(TargetKind::Missing) => InstallState::NotInstalled,
            Ok(TargetKind::Owned) => InstallState::Installed,
            Ok(TargetKind::SystemManaged) => InstallState::SystemManaged,
            Ok(TargetKind::Legacy) => InstallState::Legacy,
            Ok(TargetKind::Conflict) => InstallState::Conflict,
            Err(error) => {
                return Status {
                    state: InstallState::Unknown,
                    missing,
                    error: Some(error.to_string()),
                }
            }
        };
        Status {
            state,
            missing,
            error: None,
        }
    }

    pub fn install(self, environment: &Environment) -> Result<(), IntegrationError> {
        if environment.flatpak {
            return Err(IntegrationError::Unsupported);
        }
        let executable = crate::desktop::launch_executable()?;
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
        if self == Self::NautilusScript {
            for file in self.installed_nautilus_scripts(environment)? {
                if file.path != path {
                    fs::remove_file(&file.path)?;
                }
            }
        }
        let contents = self.contents(&executable, environment.locale)?;
        let unchanged = fs::read_to_string(&path)
            .map(|current| current == contents)
            .unwrap_or(false);
        if !unchanged {
            write_executable(&path, contents.as_bytes())?;
        }
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
        if self == Self::NautilusScript {
            if self.target_kind(&path, environment)? == TargetKind::Conflict {
                return Err(IntegrationError::Conflict);
            }
            for file in self.installed_nautilus_scripts(environment)? {
                fs::remove_file(&file.path)?;
            }
            if fs::symlink_metadata(&path).is_ok() {
                fs::remove_file(&path)?;
            }
            return Ok(());
        }
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

    /// Update an already installed integration after the desktop or app
    /// language changes, without installing a missing one.
    pub fn reconcile(self, environment: &Environment) -> Result<(), IntegrationError> {
        if environment.flatpak {
            return Ok(());
        }
        let path = self.target(environment);
        let installed = match self {
            Self::NautilusScript => !self.installed_nautilus_scripts(environment)?.is_empty(),
            _ => matches!(self.target_kind(&path, environment)?, TargetKind::Owned),
        };
        if !installed {
            return Ok(());
        }
        if matches!(self.target_kind(&path, environment)?, TargetKind::Conflict) {
            return Err(IntegrationError::Conflict);
        }
        self.install(environment)
    }
}

#[derive(Clone, Debug)]
struct InstalledScript {
    path: PathBuf,
    owned: bool,
}

/// Integrations selected by "install all" for detected file managers.
/// At most one Nautilus mode is returned so Nautilus does not show duplicate
/// entries. A legacy installation is migrated with its own mode.
pub fn bulk_install_candidates(environment: &Environment) -> Vec<FileManagerIntegration> {
    if environment.flatpak {
        return Vec::new();
    }
    let mut result = Vec::new();
    let dolphin = FileManagerIntegration::Dolphin.inspect(environment);
    if bulk_installable(&dolphin) {
        result.push(FileManagerIntegration::Dolphin);
    }

    let script = FileManagerIntegration::NautilusScript.inspect(environment);
    let menu = FileManagerIntegration::NautilusMenu.inspect(environment);
    let any_installed = [&script, &menu]
        .iter()
        .any(|status| status.state == InstallState::Installed);
    if !any_installed {
        let script_ok = bulk_installable(&script);
        let menu_ok = bulk_installable(&menu);
        if script.state == InstallState::Legacy && script_ok {
            result.push(FileManagerIntegration::NautilusScript);
        } else if menu_ok {
            result.push(FileManagerIntegration::NautilusMenu);
        } else if script_ok {
            result.push(FileManagerIntegration::NautilusScript);
        }
    }

    let nemo = FileManagerIntegration::Nemo.inspect(environment);
    if bulk_installable(&nemo) {
        result.push(FileManagerIntegration::Nemo);
    }
    result
}

fn bulk_installable(status: &Status) -> bool {
    status.missing.is_empty()
        && matches!(
            status.state,
            InstallState::NotInstalled | InstallState::Legacy
        )
}

pub fn bulk_remove_candidates(environment: &Environment) -> Vec<FileManagerIntegration> {
    if environment.flatpak {
        return Vec::new();
    }
    FileManagerIntegration::ALL
        .into_iter()
        .filter(|integration| {
            matches!(
                integration.inspect(environment).state,
                InstallState::Installed | InstallState::Legacy
            )
        })
        .collect()
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
    home: PathBuf,
    data_home: PathBuf,
    data_dirs: Vec<PathBuf>,
    search_path: Vec<PathBuf>,
    locale: Locale,
    flatpak: bool,
    /// Test/fixture override; production probes the real bridge.
    nautilus_python_available: Option<bool>,
}

impl Environment {
    pub fn current(app_language: Locale) -> io::Result<Self> {
        let home = home_directory()?;
        let data_home = xdg_data_home(&home)?;
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
            home,
            data_home,
            data_dirs,
            search_path,
            locale: i18n::integration_locale(app_language),
            flatpak: Path::new("/.flatpak-info").is_file(),
            nautilus_python_available: None,
        })
    }

    pub fn is_flatpak(&self) -> bool {
        self.flatpak
    }

    fn command_path(&self, command: &str) -> Option<PathBuf> {
        self.search_path.iter().find_map(|directory| {
            let path = directory.join(command);
            (path.is_file() && is_executable(&path)).then_some(path)
        })
    }

    fn command_exists(&self, command: &str) -> bool {
        self.command_path(command).is_some()
    }

    fn requirement_met(&self, requirement: Requirement) -> bool {
        match requirement {
            Requirement::Command { command, .. } => self.command_exists(command),
            Requirement::NautilusPython => self.nautilus_python_available.unwrap_or_else(|| {
                self.nautilus_python_bridge_exists() && self.nautilus_python_import_works()
            }),
        }
    }

    fn nautilus_python_bridge_exists(&self) -> bool {
        const LIBRARY: &str = "libnautilus-python.so";
        let relative = Path::new("nautilus").join("extensions-4").join(LIBRARY);
        let mut candidates = vec![
            PathBuf::from("/usr/lib").join(&relative),
            PathBuf::from("/usr/lib64").join(&relative),
            PathBuf::from("/usr/local/lib").join(&relative),
            self.home.join(".local/lib").join(&relative),
        ];
        for root in ["/usr/lib", "/usr/lib64", "/usr/local/lib"] {
            let Ok(entries) = fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    candidates.push(path.join(&relative));
                }
            }
        }
        candidates.into_iter().any(|path| path.is_file())
    }

    fn nautilus_python_import_works(&self) -> bool {
        let mut interpreters = Vec::new();
        let system = PathBuf::from("/usr/bin/python3");
        if system.is_file() && is_executable(&system) {
            interpreters.push(system);
        }
        if let Some(path) = self.command_path("python3") {
            if !interpreters.contains(&path) {
                interpreters.push(path);
            }
        }
        interpreters.into_iter().any(|python| {
            Command::new(python)
                .args([
                    "-c",
                    "import gi; gi.require_version('Nautilus', '4.0'); from gi.repository import Nautilus",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        })
    }

    /// Deterministic environment for the headless GTK fixture. It never reads
    /// or writes the real user's data directories.
    pub fn fixture(app_language: Locale) -> Self {
        Self {
            home: PathBuf::from("/nonexistent/localsend-gtk-home"),
            data_home: PathBuf::from("/nonexistent/localsend-gtk-test"),
            data_dirs: Vec::new(),
            search_path: Vec::new(),
            locale: i18n::integration_locale(app_language),
            flatpak: false,
            nautilus_python_available: Some(false),
        }
    }

    #[cfg(test)]
    fn for_test(root: &Path, commands: &[&str], locale: Locale, flatpak: bool) -> Self {
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
            home: root.join("home"),
            data_home: root.join("data"),
            data_dirs: vec![root.join("system")],
            search_path: vec![bin],
            locale,
            flatpak,
            nautilus_python_available: Some(false),
        }
    }
}

fn home_directory() -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_owned()))
        .ok_or_else(|| io::Error::other("Could not locate the user home directory"))
}

fn xdg_data_home(home: &Path) -> io::Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Ok(path);
    }
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

fn nemo_exec_argument(executable: &Path) -> io::Result<String> {
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
    // Nemo substitutes its own % tokens before shell parsing, so a literal
    // percent in the executable path must be doubled first.
    let text = text.replace('%', "%%").replace('\'', "'\\''");
    Ok(format!("'{text}' -- %F"))
}

fn localized_action_lines(prefix: &str, fallback: Locale) -> String {
    let mut result = format!(
        "{prefix}={}\n",
        i18n::tr_for(fallback, "Send with LocalSend")
    );
    for (locale, key) in [
        (Locale::ZhCn, "zh_CN"),
        (Locale::ZhTw, "zh_TW"),
        (Locale::Ja, "ja"),
        (Locale::Ko, "ko"),
        (Locale::De, "de"),
        (Locale::Fr, "fr"),
        (Locale::Es, "es"),
        (Locale::Ru, "ru"),
    ] {
        result.push_str(&format!(
            "{prefix}[{key}]={}\n",
            i18n::tr_for(locale, "Send with LocalSend")
        ));
    }
    result
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

fn python_string(executable: &Path) -> io::Result<String> {
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
    Ok(python_text(text.to_owned()))
}

fn python_text(text: String) -> String {
    serde_json::to_string(&text).expect("JSON string encoding")
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

    fn environment(root: &Path, commands: &[&str], locale: Locale) -> Environment {
        Environment::for_test(root, commands, locale, false)
    }

    fn target(integration: FileManagerIntegration, environment: &Environment) -> PathBuf {
        integration.target(environment)
    }

    fn legacy_script() -> &'static str {
        "#!/usr/bin/env bash\n# GNOME Nautilus / Files script to send selected files via LocalSend GTK.\nlocalsend_gtk_binary=\nexec localsend-gtk -- \"$@\"\n"
    }

    fn assert_script_syntax(path: &Path) {
        assert!(Command::new("bash")
            .args(["-n", path.to_str().unwrap()])
            .status()
            .unwrap()
            .success());
    }

    fn assert_python_syntax(path: &Path) {
        assert!(
            Command::new("python3")
                .args([
                    "-c",
                    "import sys; compile(open(sys.argv[1], encoding='utf-8').read(), sys.argv[1], 'exec')",
                    path.to_str().unwrap(),
                ])
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn installs_and_removes_script_dolphin_and_nemo_integrations() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(
            root.path(),
            &["dolphin", "nautilus", "bash", "nemo"],
            Locale::En,
        );

        for integration in [
            FileManagerIntegration::Dolphin,
            FileManagerIntegration::NautilusScript,
            FileManagerIntegration::Nemo,
        ] {
            let status = integration.inspect(&environment);
            assert_eq!(status.state, InstallState::NotInstalled);
            assert!(status.missing.is_empty(), "{integration:?}");

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
    fn nemo_action_has_localized_names_and_safe_exec() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path(), &["nemo"], Locale::Es);
        let integration = FileManagerIntegration::Nemo;
        integration.install(&environment).unwrap();
        let path = target(integration, &environment);
        assert!(path.ends_with("nemo/actions/localsend-gtk.nemo_action"));
        let contents = fs::read_to_string(&path).unwrap();
        let keyfile = glib::KeyFile::new();
        keyfile
            .load_from_data(&contents, glib::KeyFileFlags::NONE)
            .unwrap();
        assert_eq!(keyfile.string("Nemo Action", OWNER_KEY).unwrap(), APP_ID);
        assert!(contents.contains("Name[es]=Enviar con LocalSend\n"));
        assert!(contents.contains("Comment[es]=Enviar con LocalSend\n"));
        assert_eq!(keyfile.string("Nemo Action", "Quote").unwrap(), "double");
        assert_eq!(keyfile.string("Nemo Action", "Extensions").unwrap(), "any;");
        let exec = keyfile.string("Nemo Action", "Exec").unwrap();
        assert!(exec.contains("-- %F"));
        assert!(exec.contains(&env::current_exe().unwrap().to_string_lossy().into_owned()));
    }

    #[test]
    fn nautilus_python_menu_is_generated_with_a_localized_label() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path(), &["nautilus"], Locale::Es);
        let integration = FileManagerIntegration::NautilusMenu;
        integration.install(&environment).unwrap();
        let path = target(integration, &environment);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains(OWNER_COMMENT));
        assert!(contents.contains("Enviar con LocalSend"));
        assert!(!contents.contains("@LOCALSEND_GTK_"));
        assert_python_syntax(&path);
        assert_eq!(
            integration.inspect(&environment).state,
            InstallState::Installed
        );

        integration.remove(&environment).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn nautilus_script_name_and_messages_follow_the_integration_locale() {
        let root = tempfile::tempdir().unwrap();
        let spanish = environment(root.path(), &["nautilus", "bash"], Locale::Es);
        let integration = FileManagerIntegration::NautilusScript;
        integration.install(&spanish).unwrap();
        let spanish_path = target(integration, &spanish);
        assert!(spanish_path.ends_with("Enviar con LocalSend"));
        let contents = fs::read_to_string(&spanish_path).unwrap();
        assert!(contents.contains("Solo se pueden enviar archivos y carpetas locales válidos."));
        assert_script_syntax(&spanish_path);

        let english = environment(root.path(), &["nautilus", "bash"], Locale::En);
        integration.reconcile(&english).unwrap();
        let english_path = target(integration, &english);
        assert!(english_path.ends_with("Send with LocalSend"));
        assert!(!spanish_path.exists());
        let contents = fs::read_to_string(&english_path).unwrap();
        assert!(contents.contains("Only valid local files and folders can be sent."));

        integration.remove(&english).unwrap();
        let legacy_path = root
            .path()
            .join("data/nautilus/scripts/Send with LocalSend");
        fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        fs::write(&legacy_path, legacy_script()).unwrap();
        assert_eq!(integration.inspect(&spanish).state, InstallState::Legacy);
        integration.reconcile(&spanish).unwrap();
        assert!(!legacy_path.exists());
        assert!(spanish_path.exists());
    }

    #[test]
    fn dolphin_menu_localizes_its_fallback_name() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path(), &["dolphin"], Locale::Es);
        let integration = FileManagerIntegration::Dolphin;
        integration.install(&environment).unwrap();
        let contents = fs::read_to_string(target(integration, &environment)).unwrap();
        assert!(contents
            .lines()
            .any(|line| line == "Name=Enviar con LocalSend"));
        let keyfile = glib::KeyFile::new();
        keyfile
            .load_from_data(&contents, glib::KeyFileFlags::NONE)
            .unwrap();
        let exec = keyfile
            .string("Desktop Action SendWithLocalSend", "Exec")
            .unwrap();
        assert!(exec.ends_with(" %U"));
        assert!(exec.contains(&env::current_exe().unwrap().to_string_lossy().into_owned()));
    }

    #[test]
    fn unusual_executable_paths_are_escaped_for_each_integration() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("Local Send '$`\\\";%f青");
        let dolphin = FileManagerIntegration::Dolphin
            .contents(&executable, Locale::En)
            .unwrap();
        assert!(dolphin.contains("/usr/bin/env -- "));
        assert!(dolphin.contains("%%f"));

        let script = FileManagerIntegration::NautilusScript
            .contents(&executable, Locale::En)
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

        let menu = FileManagerIntegration::NautilusMenu
            .contents(&executable, Locale::En)
            .unwrap();
        assert!(menu.contains("BINARY = "));
        assert!(menu.contains("\\\""));

        let nemo = FileManagerIntegration::Nemo
            .contents(&executable, Locale::En)
            .unwrap();
        assert!(nemo.contains("-- %F"));
        assert!(nemo.contains("%%f"));
        assert!(nemo.contains("'\\''"));
    }

    #[test]
    fn unowned_files_are_reported_and_never_replaced_or_deleted() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path(), &["dolphin", "nautilus", "bash"], Locale::En);
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
        let environment = environment(root.path(), &["dolphin", "nautilus", "bash"], Locale::En);
        for (integration, legacy) in [
            (
                FileManagerIntegration::Dolphin,
                "[Desktop Entry]\nType=Service\nActions=SendWithLocalSend;\nIcon=/home/user/.local/share/icons/hicolor/512x512/apps/localsend-gtk.png\n\n[Desktop Action SendWithLocalSend]\nName=Send with LocalSend\nExec=/home/user/.local/bin/localsend-gtk %U\n",
            ),
            (FileManagerIntegration::NautilusScript, legacy_script()),
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
        let environment = environment(root.path(), &["nautilus", "bash"], Locale::En);
        let integration = FileManagerIntegration::NautilusScript;
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
        let environment = environment(root.path(), &["nautilus", "bash"], Locale::En);
        let integration = FileManagerIntegration::NautilusScript;
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
        let environment = environment(root.path(), &["dolphin"], Locale::En);
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
    fn missing_requirements_name_the_packages_to_install() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path(), &[], Locale::En);
        let status = FileManagerIntegration::NautilusScript.inspect(&environment);
        assert_eq!(
            status
                .missing
                .iter()
                .map(|requirement| requirement.package())
                .collect::<Vec<_>>(),
            ["nautilus", "bash"]
        );
        assert!(FileManagerIntegration::NautilusMenu
            .requirements()
            .iter()
            .any(|requirement| matches!(requirement, Requirement::NautilusPython)));
        assert_eq!(Requirement::NautilusPython.package(), "nautilus-python");
        let status = FileManagerIntegration::NautilusMenu.inspect(&environment);
        assert_eq!(
            status
                .missing
                .iter()
                .map(|requirement| requirement.package())
                .collect::<Vec<_>>(),
            ["nautilus", "nautilus-python"]
        );

        let mut available = environment.clone();
        available.nautilus_python_available = Some(true);
        let status = FileManagerIntegration::NautilusMenu.inspect(&available);
        assert_eq!(
            status
                .missing
                .iter()
                .map(|requirement| requirement.package())
                .collect::<Vec<_>>(),
            ["nautilus"]
        );
    }

    #[test]
    fn bulk_install_prefers_direct_menu_and_skips_missing_managers() {
        let root = tempfile::tempdir().unwrap();
        let mut environment = environment(
            root.path(),
            &["dolphin", "nautilus", "bash", "nemo"],
            Locale::En,
        );
        environment.nautilus_python_available = Some(true);
        assert_eq!(
            bulk_install_candidates(&environment),
            [
                FileManagerIntegration::Dolphin,
                FileManagerIntegration::NautilusMenu,
                FileManagerIntegration::Nemo,
            ]
        );

        environment.nautilus_python_available = Some(false);
        assert_eq!(
            bulk_install_candidates(&environment),
            [
                FileManagerIntegration::Dolphin,
                FileManagerIntegration::NautilusScript,
                FileManagerIntegration::Nemo,
            ]
        );

        let no_nemo_root = tempfile::tempdir().unwrap();
        let without_nemo = Environment::for_test(
            no_nemo_root.path(),
            &["dolphin", "nautilus", "bash"],
            Locale::En,
            false,
        );
        assert_eq!(
            bulk_install_candidates(&without_nemo),
            [
                FileManagerIntegration::Dolphin,
                FileManagerIntegration::NautilusScript,
            ]
        );

        FileManagerIntegration::NautilusMenu
            .install(&environment)
            .unwrap();
        assert!(!bulk_install_candidates(&environment)
            .iter()
            .any(|integration| matches!(
                *integration,
                FileManagerIntegration::NautilusScript | FileManagerIntegration::NautilusMenu
            )));
    }

    #[test]
    fn bulk_remove_collects_owned_modes_and_skips_foreign_files() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(
            root.path(),
            &["dolphin", "nautilus", "bash", "nemo"],
            Locale::En,
        );
        for integration in [
            FileManagerIntegration::Dolphin,
            FileManagerIntegration::NautilusScript,
            FileManagerIntegration::NautilusMenu,
            FileManagerIntegration::Nemo,
        ] {
            integration.install(&environment).unwrap();
        }
        assert_eq!(
            bulk_remove_candidates(&environment),
            [
                FileManagerIntegration::Dolphin,
                FileManagerIntegration::NautilusScript,
                FileManagerIntegration::NautilusMenu,
                FileManagerIntegration::Nemo,
            ]
        );

        let conflict = environment.clone();
        let path = FileManagerIntegration::Dolphin.target(&conflict);
        fs::remove_file(&path).unwrap();
        fs::write(&path, "foreign").unwrap();
        assert!(!bulk_remove_candidates(&conflict).contains(&FileManagerIntegration::Dolphin));
    }

    #[test]
    fn flatpak_rejects_all_installations() {
        let root = tempfile::tempdir().unwrap();
        for integration in FileManagerIntegration::ALL {
            let environment = Environment::for_test(
                root.path(),
                &["dolphin", "nautilus", "bash"],
                Locale::En,
                true,
            );
            assert!(matches!(
                integration.install(&environment),
                Err(IntegrationError::Unsupported)
            ));
        }
    }

    #[test]
    fn targets_follow_the_localized_xdg_layout() {
        let english = Environment::for_test(Path::new("/tmp/root"), &[], Locale::En, false);
        let spanish = Environment::for_test(Path::new("/tmp/root"), &[], Locale::Es, false);
        assert_eq!(
            FileManagerIntegration::Dolphin.target(&english),
            Path::new("/tmp/root/data/kio/servicemenus/localsend-dolphin.desktop")
        );
        assert_eq!(
            FileManagerIntegration::NautilusScript.target(&english),
            Path::new("/tmp/root/data/nautilus/scripts/Send with LocalSend")
        );
        assert_eq!(
            FileManagerIntegration::NautilusScript.target(&spanish),
            Path::new("/tmp/root/data/nautilus/scripts/Enviar con LocalSend")
        );
        assert_eq!(
            FileManagerIntegration::NautilusMenu.target(&english),
            Path::new("/tmp/root/data/nautilus-python/extensions/localsend-gtk.py")
        );
        assert_eq!(
            FileManagerIntegration::Nemo.target(&english),
            Path::new("/tmp/root/data/nemo/actions/localsend-gtk.nemo_action")
        );
    }
}
