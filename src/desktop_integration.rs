//! Optional application-menu integration for the AppImage build.
//!
//! An AppImage is a single file, so the desktop environment cannot show its
//! icon or launcher until a `.desktop` entry and icon are installed for the
//! user. This module only manages files owned by LocalSend GTK.
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
const ICON_NAME: &str = APP_ID;
const LOGO: &[u8] = include_bytes!("../assets/logo.png");
const DESKTOP_FILE: &str = "org.localsend.localsend_gtk.desktop";

#[derive(Clone, Debug)]
pub struct Context {
    data_home: PathBuf,
    appimage: PathBuf,
}

impl Context {
    pub fn current() -> Option<Self> {
        let appimage = appimage_path()?;
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_owned()))?;
        let data_home = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        Some(Self {
            data_home,
            appimage,
        })
    }

    fn desktop_path(&self) -> PathBuf {
        self.data_home.join("applications").join(DESKTOP_FILE)
    }

    fn icon_path(&self) -> PathBuf {
        self.data_home
            .join("icons/hicolor/512x512/apps")
            .join(format!("{ICON_NAME}.png"))
    }
}

pub fn appimage_path() -> Option<PathBuf> {
    env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    NotInstalled,
    Installed,
    Conflict,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub state: State,
    pub error: Option<String>,
}

pub fn inspect(context: &Context) -> Status {
    let desktop = context.desktop_path();
    match fs::read_to_string(&desktop) {
        Ok(contents) => {
            let keyfile = glib::KeyFile::new();
            if keyfile
                .load_from_data(&contents, glib::KeyFileFlags::NONE)
                .is_err()
            {
                return Status {
                    state: State::Conflict,
                    error: None,
                };
            }
            if keyfile.string("Desktop Entry", OWNER_KEY).ok().as_deref() == Some(APP_ID) {
                Status {
                    state: State::Installed,
                    error: None,
                }
            } else {
                Status {
                    state: State::Conflict,
                    error: None,
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Status {
            state: State::NotInstalled,
            error: None,
        },
        Err(error) => Status {
            state: State::Unknown,
            error: Some(error.to_string()),
        },
    }
}

pub fn install(context: &Context, app_language: Locale) -> Result<(), Error> {
    let desktop = context.desktop_path();
    if let Ok(contents) = fs::read_to_string(&desktop) {
        let keyfile = glib::KeyFile::new();
        let owned = keyfile
            .load_from_data(&contents, glib::KeyFileFlags::NONE)
            .is_ok()
            && keyfile.string("Desktop Entry", OWNER_KEY).ok().as_deref() == Some(APP_ID);
        if !owned {
            return Err(Error::Conflict);
        }
    } else if fs::symlink_metadata(&desktop).is_ok() {
        return Err(Error::Conflict);
    }
    let icon = context.icon_path();
    if let Ok(bytes) = fs::read(&icon) {
        if bytes != LOGO {
            return Err(Error::Conflict);
        }
    } else if fs::symlink_metadata(&icon).is_ok() {
        return Err(Error::Conflict);
    }

    let exec = format!("{} %U", desktop_exec_argument(&context.appimage)?);
    let action_names = action_names(app_language);
    let contents = format!(
        "[Desktop Entry]\nType=Application\nName=LocalSend GTK\nGenericName=File Transfer\nComment=An open source cross-platform alternative to AirDrop (GTK4/Wayland)\nExec={exec}\nIcon={ICON_NAME}\nTerminal=false\nStartupNotify=true\nStartupWMClass={APP_ID}\nCategories=Network;FileTransfer;Utility;\nMimeType=application/octet-stream;inode/directory;\nKeywords=localsend;airdrop;transfer;sharing;lan;network;gtk;\nX-Wayland-Native=true\nActions=SendWithLocalSend;\n{OWNER_KEY}={APP_ID}\n\n[Desktop Action SendWithLocalSend]\n{action_names}Exec={exec}\n",
    );
    write_file(&icon, LOGO)?;
    write_file(&desktop, contents.as_bytes())?;
    update_caches(context);
    Ok(())
}

pub fn remove(context: &Context) -> Result<(), Error> {
    let desktop = context.desktop_path();
    let icon = context.icon_path();
    let desktop_owned = fs::read_to_string(&desktop)
        .ok()
        .map(|contents| {
            let keyfile = glib::KeyFile::new();
            keyfile
                .load_from_data(&contents, glib::KeyFileFlags::NONE)
                .is_ok()
                && keyfile.string("Desktop Entry", OWNER_KEY).ok().as_deref() == Some(APP_ID)
        })
        .unwrap_or(false);
    if fs::symlink_metadata(&desktop).is_ok() && !desktop_owned {
        return Err(Error::Conflict);
    }
    let icon_owned = fs::read(&icon).map(|bytes| bytes == LOGO).unwrap_or(false);
    if fs::symlink_metadata(&icon).is_ok() && !icon_owned {
        return Err(Error::Conflict);
    }
    if fs::symlink_metadata(&desktop).is_ok() {
        fs::remove_file(&desktop)?;
    }
    if fs::symlink_metadata(&icon).is_ok() {
        fs::remove_file(&icon)?;
    }
    update_caches(context);
    Ok(())
}

/// Refresh an installed launcher after a language change.
pub fn reconcile(context: &Context, app_language: Locale) -> Result<(), Error> {
    if inspect(context).state == State::Installed {
        install(context, app_language)?;
    }
    Ok(())
}

fn action_names(app_language: Locale) -> String {
    let fallback = i18n::tr_for(
        i18n::integration_locale(app_language),
        "Send with LocalSend",
    );
    let mut result = format!("Name={fallback}\n");
    for locale in Locale::ALL {
        let Some(key) = desktop_locale(locale) else {
            continue;
        };
        result.push_str(&format!(
            "Name[{key}]={}\n",
            i18n::tr_for(locale, "Send with LocalSend")
        ));
    }
    result
}

fn desktop_locale(locale: Locale) -> Option<&'static str> {
    match locale {
        Locale::ZhCn => Some("zh_CN"),
        Locale::ZhTw => Some("zh_TW"),
        Locale::Ja => Some("ja"),
        Locale::Ko => Some("ko"),
        Locale::De => Some("de"),
        Locale::Fr => Some("fr"),
        Locale::Es => Some("es"),
        Locale::Ru => Some("ru"),
        Locale::System | Locale::En => None,
    }
}

fn write_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing desktop integration directory"))?;
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
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o644))?;
        }
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn update_caches(context: &Context) {
    let applications = context.data_home.join("applications");
    let icons = context.data_home.join("icons/hicolor");
    std::thread::spawn(move || {
        let _ = Command::new("update-desktop-database")
            .arg(applications)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = Command::new("gtk-update-icon-cache")
            .args(["-f", "-t"])
            .arg(icons)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    });
}

#[derive(Debug)]
pub enum Error {
    Conflict,
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict => write!(
                formatter,
                "A file not created by LocalSend already uses this name."
            ),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(root: &Path) -> Context {
        let appimage = root.join("LocalSend GTK.AppImage");
        fs::write(&appimage, b"appimage").unwrap();
        Context {
            data_home: root.join("data"),
            appimage,
        }
    }

    #[test]
    fn installs_and_removes_a_user_launcher_and_icon() {
        let root = tempfile::tempdir().unwrap();
        let context = context(root.path());
        assert_eq!(inspect(&context).state, State::NotInstalled);
        install(&context, Locale::Es).unwrap();
        assert_eq!(inspect(&context).state, State::Installed);
        let desktop = fs::read_to_string(context.desktop_path()).unwrap();
        assert!(desktop.contains(&format!("Exec=\"{}\" %U", context.appimage.display())));
        assert!(desktop.contains(&format!("{OWNER_KEY}={APP_ID}")));
        assert!(desktop.contains("Name[es]=Enviar con LocalSend"));
        assert_eq!(fs::read(context.icon_path()).unwrap(), LOGO);
        remove(&context).unwrap();
        assert_eq!(inspect(&context).state, State::NotInstalled);
    }

    #[test]
    fn foreign_files_are_never_overwritten_or_removed() {
        let root = tempfile::tempdir().unwrap();
        let context = context(root.path());
        fs::create_dir_all(context.desktop_path().parent().unwrap()).unwrap();
        fs::write(context.desktop_path(), "foreign").unwrap();
        assert_eq!(inspect(&context).state, State::Conflict);
        assert!(matches!(
            install(&context, Locale::En),
            Err(Error::Conflict)
        ));
        assert!(matches!(remove(&context), Err(Error::Conflict)));
        assert_eq!(
            fs::read_to_string(context.desktop_path()).unwrap(),
            "foreign"
        );
    }

    #[test]
    fn reconcile_updates_the_localized_action_name() {
        let root = tempfile::tempdir().unwrap();
        let context = context(root.path());
        install(&context, Locale::En).unwrap();
        reconcile(&context, Locale::Es).unwrap();
        let desktop = fs::read_to_string(context.desktop_path()).unwrap();
        assert!(desktop.contains("Name=Enviar con LocalSend"));
    }
}
