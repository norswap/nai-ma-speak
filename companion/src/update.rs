//! Updates. The addon and the companion only understand each other at the same
//! version, so the companion looks after both: it knows the releases, the version in
//! the addon's folder and the version the game holds, and puts a release in place,
//! first the addon, then itself.

use crate::protocol::AddonVersion;
use crate::worker::{UiEvent, UiSender};
use serde::Deserialize;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const RELEASES: &str = "https://api.github.com/repos/norswap/nai-ma-speak/releases";
const ADDON_FILE: &str = "Nai-Ma-Speak-addon.zip";
const COMPANION_FILE: &str = if cfg!(target_os = "macos") {
    "Nai-Ma-Speak-macos.zip"
} else if cfg!(windows) {
    "nai-ma-speak-windows-x64.exe"
} else {
    "nai-ma-speak-linux-x64"
};
/// The addon's folder inside the game's AddOns folder, which its .toc file and its file
/// of saved settings are named after.
const ADDON_FOLDER: &str = "NaiMaSpeak";
/// What the folder was named before 0.4.0. So plain a name may be another addon's, so
/// such a folder is ours only if its .toc carries our title.
const OLD_FOLDER: &str = "Translate";
const TITLE: &str = "## Title: Nai Ma Speak";
/// The first release that can update itself. Going back further would leave a
/// companion with no way forward, so earlier releases are not offered.
pub const OLDEST_OFFERED: Version = Version([0, 4, 0]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version([u32; 3]);

impl Version {
    /// Reads "1.2.3" or "v1.2.3". A test release such as "1.2.3-beta.1" is none.
    pub fn parse(text: &str) -> Option<Self> {
        let numbers: Vec<u32> = text.trim().trim_start_matches('v').split('.').map(str::parse).collect::<Result<_, _>>().ok()?;
        Some(Version(numbers.try_into().ok()?))
    }

    /// The version of this program.
    pub fn own() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("the package version is three numbers")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let [major, minor, patch] = self.0;
        write!(f, "{major}.{minor}.{patch}")
    }
}

/// A file of a release, as GitHub lists it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct File {
    name: String,
    size: u64,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct Listed {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<File>,
}

/// A release with the addon and this system's companion.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    addon: File,
    companion: File,
}

/// The releases in GitHub's list that can be installed here, newest first.
fn releases(json: &str) -> Result<Vec<Release>, String> {
    let listed: Vec<Listed> = serde_json::from_str(json).map_err(|e| format!("the list of releases could not be read: {e}"))?;
    let mut releases: Vec<Release> = listed
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            let file = |name: &str| release.assets.iter().find(|file| file.name == name).cloned();
            Some(Release { version: Version::parse(&release.tag_name)?, addon: file(ADDON_FILE)?, companion: file(COMPANION_FILE)? })
        })
        .collect();
    releases.sort_by_key(|release| std::cmp::Reverse(release.version));
    Ok(releases)
}

/// What the game's AddOns folder holds of the addon.
#[derive(Clone, Debug, PartialEq)]
pub enum Addon {
    /// No AddOns folder is known, so there is nowhere to look.
    Unknown,
    /// The AddOns folder holds no addon.
    Missing,
    /// A link to a folder elsewhere, as a developer sets up, and the version there. It is
    /// replaced only when the player asks for the update, never by an automatic one.
    Linked(Option<Version>),
    /// Its version, if the .toc names one.
    Installed(Option<Version>),
}

impl Addon {
    /// The version of the addon's files, if there are any and they name one.
    pub fn version(&self) -> Option<Version> {
        match self {
            Addon::Linked(version) | Addon::Installed(version) => *version,
            Addon::Unknown | Addon::Missing => None,
        }
    }
}

/// The .toc file of the addon in the folder `name`, if there is one.
fn toc(addons: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(addons.join(name).join(format!("{name}.toc"))).ok()
}

/// The addon's folder under its old name, if it is there and ours.
fn old_addon(addons: &Path) -> Option<PathBuf> {
    let ours = toc(addons, OLD_FOLDER)?.lines().any(|line| line.trim() == TITLE);
    ours.then(|| addons.join(OLD_FOLDER))
}

/// What the AddOns folder holds of the addon, under its name or, failing that, its old one.
pub fn addon_in(addons: &Path) -> Addon {
    let there = |name: &str| std::fs::symlink_metadata(addons.join(name)).ok();
    let (name, found) = match (there(ADDON_FOLDER), old_addon(addons).and_then(|_| there(OLD_FOLDER))) {
        (Some(found), _) => (ADDON_FOLDER, found),
        (None, Some(found)) => (OLD_FOLDER, found),
        (None, None) => return Addon::Missing,
    };
    let toc = toc(addons, name).unwrap_or_default();
    let version = toc.lines().find_map(|line| Version::parse(line.strip_prefix("## Version:")?));
    if found.is_symlink() {
        Addon::Linked(version)
    } else {
        Addon::Installed(version)
    }
}

/// The AddOns folders of the games installed in the usual places: one per kind of
/// game (`_retail_`, `_classic_` and so on).
pub fn addon_folders() -> Vec<PathBuf> {
    let installs: &[&str] = if cfg!(target_os = "macos") {
        &["/Applications/World of Warcraft"]
    } else if cfg!(windows) {
        &["C:\\Program Files (x86)\\World of Warcraft", "C:\\Program Files\\World of Warcraft"]
    } else {
        &[]
    };
    let mut folders: Vec<PathBuf> = installs
        .iter()
        .filter_map(|install| std::fs::read_dir(install).ok())
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path().join("Interface").join("AddOns")))
        .filter(|folder| folder.is_dir())
        .collect();
    folders.sort();
    folders
}

/// The AddOns folder to look after: the one in the settings, or else the one found
/// that holds the addon, or else the only one found.
pub fn addons_folder(configured: &str, found: &[PathBuf]) -> Option<PathBuf> {
    if !configured.trim().is_empty() {
        return Some(PathBuf::from(configured.trim()));
    }
    let with_addon = found.iter().find(|folder| addon_in(folder) != Addon::Missing);
    with_addon.or(found.first().filter(|_| found.len() == 1)).cloned()
}

/// What there is to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Advice {
    Nothing,
    /// Put this release in place: the addon, then the companion.
    Update(Version),
    /// The companion is as it should be; the addon's folder holds another version, or none.
    InstallAddon,
    /// The folder holds the right addon and the game still runs another: /reload.
    Reload,
    /// The game runs the right Lua files but started with another version's list of them.
    RestartGame,
}

/// `game` is what the addon in the game said, and `silent` that it has been talking
/// for a while without saying its version, as addons from before 0.4.0 do.
pub fn advice(own: Version, target: Option<Version>, addon: &Addon, game: Option<&AddonVersion>, silent: bool) -> Advice {
    if let Some(target) = target.filter(|target| *target != own) {
        return Advice::Update(target);
    }
    let is_own = |version: &str| Version::parse(version) == Some(own);
    match (addon, game) {
        // Files that are gone are put back even while the game still runs them from
        // memory. With no folder known, a game that runs the addon has it somewhere.
        (Addon::Missing, _) | (Addon::Unknown, None) => Advice::InstallAddon,
        (Addon::Installed(version) | Addon::Linked(version), _) if *version != Some(own) => Advice::InstallAddon,
        (_, Some(game)) if !is_own(&game.running) => Advice::Reload,
        (_, Some(game)) if !is_own(&game.listed) => Advice::RestartGame,
        (_, None) if silent => Advice::Reload,
        _ => Advice::Nothing,
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout(Duration::from_secs(300)).build()
}

/// Asks GitHub for the releases and sends them to the window.
pub fn check(ui: UiSender) {
    std::thread::spawn(move || {
        let listed = agent()
            .get(RELEASES)
            .set("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| format!("GitHub could not be asked for the releases: {e}"))
            .and_then(|reply| reply.into_string().map_err(|e| e.to_string()))
            .and_then(|json| releases(&json));
        ui.send(UiEvent::Releases(listed));
    });
}

fn download(file: &File) -> Result<Vec<u8>, String> {
    let problem = |e: &dyn fmt::Display| format!("{} could not be downloaded: {e}", file.name);
    let reply = agent().get(&file.browser_download_url).call().map_err(|e| problem(&e))?;
    let mut bytes = Vec::new();
    reply.into_reader().take(file.size + 1).read_to_end(&mut bytes).map_err(|e| problem(&e))?;
    if bytes.len() as u64 != file.size {
        return Err(problem(&format!("it came as {} bytes, not the {} listed", bytes.len(), file.size)));
    }
    Ok(bytes)
}

fn unzip(bytes: &[u8], into: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    archive.extract(into).map_err(|e| e.to_string())
}

/// `path` with more at the end of its name: "Translate" and ".old" make "Translate.old".
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(suffix);
    path.with_file_name(name)
}

/// Removes a file or a folder; one that is not there is no problem.
fn remove(path: &Path) {
    let _ = if path.is_dir() && !path.is_symlink() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
}

/// Puts `new` in the place of `target`. What was there moves aside first, since a
/// running program cannot be written over on Windows but can be renamed, and comes
/// back if `new` cannot take its place. What stays aside is removed by `clean_up`.
fn swap(target: &Path, new: &Path) -> Result<(), String> {
    let problem = |e: std::io::Error| format!("{} could not be replaced: {e}", target.display());
    let old = beside(target, ".old");
    remove(&old);
    // Not `exists`, which asks about what a link leads to: a link is moved aside itself.
    let had = std::fs::symlink_metadata(target).is_ok();
    if had {
        std::fs::rename(target, &old).map_err(problem)?;
    }
    if let Err(e) = std::fs::rename(new, target) {
        if had {
            let _ = std::fs::rename(&old, target);
        }
        return Err(problem(e));
    }
    remove(&old);
    Ok(())
}

/// Replaces the addon in the game's AddOns folder with the one in a release's zip.
fn install_addon(zip: &[u8], addons: &Path) -> Result<(), String> {
    if !addons.is_dir() {
        return Err(format!("there is no folder {}", addons.display()));
    }
    let unpacked = addons.join(format!("{ADDON_FOLDER}.new"));
    remove(&unpacked);
    let installed = unzip(zip, &unpacked).and_then(|()| swap(&addons.join(ADDON_FOLDER), &unpacked.join(ADDON_FOLDER)));
    remove(&unpacked);
    if installed.is_ok() {
        retire_old(addons);
    }
    installed
}

/// Removes the addon under its old name, which the game would load beside the new one,
/// after handing its saved settings on: the game keeps them in a file named after the
/// addon, one per account, so each is copied to the new name unless that is there already.
fn retire_old(addons: &Path) {
    let Some(old) = old_addon(addons) else {
        return;
    };
    let accounts = addons.parent().and_then(Path::parent).map(|game| game.join("WTF").join("Account"));
    for account in accounts.and_then(|accounts| std::fs::read_dir(accounts).ok()).into_iter().flatten().flatten() {
        let saved = account.path().join("SavedVariables");
        let (from, to) = (saved.join(format!("{OLD_FOLDER}.lua")), saved.join(format!("{ADDON_FOLDER}.lua")));
        if let (Ok(settings), false) = (std::fs::read_to_string(&from), to.exists()) {
            let _ = std::fs::write(&to, settings.replace("TranslateDB", "NaiMaSpeakDB"));
        }
    }
    remove(&old);
}

/// What a release's companion replaces: the app bundle on macOS, the program elsewhere.
fn own_place() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") {
        let bundle = crate::config::app_bundle(&exe).ok_or("the companion does not run from an app bundle, so it cannot replace itself")?;
        return Ok(bundle.to_path_buf());
    }
    Ok(exe)
}

/// Replaces the companion at `place` with a release's: the app inside its zip on
/// macOS, the program itself elsewhere. The running copy goes on until it quits.
fn install_companion(file: &[u8], place: &Path) -> Result<(), String> {
    let new = beside(place, ".new");
    remove(&new);
    let installed = if cfg!(target_os = "macos") {
        unzip(file, &new).and_then(|()| {
            let apps = std::fs::read_dir(&new).map_err(|e| e.to_string())?;
            let app = apps.filter_map(|entry| Some(entry.ok()?.path())).find(|path| path.extension().is_some_and(|extension| extension == "app"));
            swap(place, &app.ok_or("the download holds no app")?)
        })
    } else {
        std::fs::write(&new, file).and_then(|()| make_runnable(&new)).map_err(|e| e.to_string()).and_then(|()| swap(place, &new))
    };
    remove(&new);
    installed
}

fn make_runnable(_program: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(_program, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// Removes what an update left aside. The companion calls this when it starts: the
/// copy it replaced has quit by then.
pub fn clean_up() {
    if let Ok(place) = own_place() {
        remove(&beside(&place, ".old"));
    }
}

/// What to put in place from a release.
pub struct Plan {
    pub release: Release,
    /// The AddOns folder to put the addon into, if it is to be replaced.
    pub addons: Option<PathBuf>,
    pub companion: bool,
}

/// Carries out a plan away from the window, telling it each step and how it ended.
/// The addon comes first: should the companion's turn fail, the old companion says
/// that the addon is ahead, instead of a new companion finding an old addon.
pub fn install(plan: Plan, ui: UiSender) {
    std::thread::spawn(move || {
        let step = |text: &str| ui.send(UiEvent::UpdateStep(text.into()));
        let run = || -> Result<(), String> {
            if let Some(addons) = &plan.addons {
                step("Downloading the addon");
                let zip = download(&plan.release.addon)?;
                step("Replacing the addon");
                install_addon(&zip, addons)?;
            }
            if plan.companion {
                step("Downloading the companion");
                let file = download(&plan.release.companion)?;
                step("Replacing the companion");
                install_companion(&file, &own_place()?)?;
            }
            Ok(())
        };
        let done = run();
        if let Err(problem) = &done {
            crate::diag::log(format!("update to {}: {problem}", plan.release.version));
        }
        ui.send(UiEvent::Updated { version: plan.release.version, companion: plan.companion, result: done });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A folder of its own for a test, empty.
    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("nai-ma-speak-test-{}-{name}", std::process::id()));
        remove(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn zipped(files: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, text) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn game(running: &str, listed: &str) -> AddonVersion {
        AddonVersion { running: running.into(), listed: listed.into() }
    }

    #[test]
    fn versions_are_three_numbers_and_compare_as_numbers() {
        assert_eq!(Version::parse("v0.10.2"), Some(Version([0, 10, 2])));
        assert_eq!(Version::parse(" 1.2.3 ").unwrap().to_string(), "1.2.3");
        assert!(Version::parse("0.10.0") > Version::parse("0.9.9"));
        for none in ["", "1.2", "1.2.3.4", "1.2.3-beta.1", "one.two.three"] {
            assert_eq!(Version::parse(none), None, "{none}");
        }
        assert!(Version::own() >= Version([0, 3, 0]));
    }

    #[test]
    fn only_whole_releases_are_listed_newest_first() {
        let file = |name: &str| format!(r#"{{"name":"{name}","size":7,"browser_download_url":"https://example.com/{name}"}}"#);
        let whole = format!("{},{}", file(ADDON_FILE), file(COMPANION_FILE));
        let json = format!(
            r#"[{{"tag_name":"v0.4.0","assets":[{whole}]}},
                {{"tag_name":"v0.10.0","assets":[{whole}],"body":"notes"}},
                {{"tag_name":"v0.11.0","draft":true,"assets":[{whole}]}},
                {{"tag_name":"v0.12.0-beta.1","prerelease":true,"assets":[{whole}]}},
                {{"tag_name":"v0.9.0","assets":[{}]}},
                {{"tag_name":"nightly","assets":[{whole}]}}]"#,
            file(ADDON_FILE)
        );
        let releases = releases(&json).unwrap();
        let versions: Vec<String> = releases.iter().map(|release| release.version.to_string()).collect();
        assert_eq!(versions, ["0.10.0", "0.4.0"]);
        assert_eq!(releases[0].companion.browser_download_url, format!("https://example.com/{COMPANION_FILE}"));
        assert!(super::releases("<html>").is_err());
    }

    #[test]
    fn says_what_there_is_to_do() {
        let own = Version([0, 4, 0]);
        let right = Addon::Installed(Some(own));
        let older = Addon::Installed(Some(Version([0, 3, 0])));
        let newer = Some(Version([0, 5, 0]));
        assert_eq!(advice(own, newer, &right, None, false), Advice::Update(Version([0, 5, 0])));
        assert_eq!(advice(own, Some(Version([0, 3, 0])), &right, None, false), Advice::Update(Version([0, 3, 0])), "going back");
        assert_eq!(advice(own, Some(own), &older, Some(&game("0.3.0", "0.3.0")), false), Advice::InstallAddon);
        assert_eq!(advice(own, None, &Addon::Installed(None), None, false), Advice::InstallAddon);
        assert_eq!(advice(own, None, &Addon::Missing, None, false), Advice::InstallAddon);
        assert_eq!(advice(own, None, &Addon::Unknown, None, false), Advice::InstallAddon);
        // The game runs the addon from a folder the companion does not know.
        assert_eq!(advice(own, None, &Addon::Unknown, Some(&game("0.4.0", "0.4.0")), false), Advice::Nothing);
        // The files were removed while the game runs: it still has them in memory.
        assert_eq!(advice(own, None, &Addon::Missing, Some(&game("0.4.0", "0.4.0")), false), Advice::InstallAddon);
        assert_eq!(advice(own, Some(own), &right, Some(&game("0.3.0", "0.3.0")), false), Advice::Reload);
        assert_eq!(advice(own, Some(own), &right, Some(&game("0.4.0", "0.3.0")), false), Advice::RestartGame);
        assert_eq!(advice(own, Some(own), &right, None, true), Advice::Reload, "an addon too old to say its version");
        assert_eq!(advice(own, Some(own), &Addon::Linked(Some(own)), Some(&game("0.3.0", "0.3.0")), false), Advice::Reload);
        assert_eq!(advice(own, Some(own), &right, Some(&game("0.4.0", "0.4.0")), false), Advice::Nothing);
        assert_eq!(advice(own, Some(own), &right, None, false), Advice::Nothing, "the game is not running");
    }

    #[test]
    fn replaces_the_addon_in_its_folder() {
        let addons = scratch("addon");
        assert_eq!(addon_in(&addons), Addon::Missing);
        let old = addons.join(ADDON_FOLDER);
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("NaiMaSpeak.toc"), "## Title: Nai Ma Speak\n## Version: 0.3.0\n").unwrap();
        std::fs::write(old.join("Gone.lua"), "").unwrap();
        assert_eq!(addon_in(&addons), Addon::Installed(Some(Version([0, 3, 0]))));

        let zip = zipped(&[("NaiMaSpeak/NaiMaSpeak.toc", "## Version: 0.4.0\n"), ("NaiMaSpeak/Core.lua", "-- new")]);
        install_addon(&zip, &addons).unwrap();
        assert_eq!(addon_in(&addons), Addon::Installed(Some(Version([0, 4, 0]))));
        assert_eq!(std::fs::read_to_string(old.join("Core.lua")).unwrap(), "-- new");
        assert!(!old.join("Gone.lua").exists(), "files of the old version are gone");
        let left: Vec<_> = std::fs::read_dir(&addons).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(left, [ADDON_FOLDER], "nothing is left beside it");

        // A download that is no addon leaves the one installed.
        assert!(install_addon(&zipped(&[("Other/file", "")]), &addons).is_err());
        assert!(install_addon(b"not a zip", &addons).is_err());
        assert_eq!(addon_in(&addons), Addon::Installed(Some(Version([0, 4, 0]))));
        assert_eq!(addons_folder("", &[scratch("empty"), addons.clone()]), Some(addons.clone()));
        assert_eq!(addons_folder(" /games/AddOns ", std::slice::from_ref(&addons)), Some(PathBuf::from("/games/AddOns")));
        assert_eq!(addons_folder("", &[scratch("one"), scratch("two")]), None, "no telling which game");
        remove(&addons);
    }

    /// Before 0.4.0 the addon's folder was named Translate.
    #[test]
    fn takes_the_place_of_the_addon_under_its_old_name() {
        let game = scratch("renamed");
        let addons = game.join("Interface").join("AddOns");
        let saved = game.join("WTF").join("Account").join("SOMEONE").join("SavedVariables");
        std::fs::create_dir_all(&saved).unwrap();
        std::fs::write(saved.join("Translate.lua"), "\nTranslateDB = {\n[\"allLines\"] = true,\n}\n").unwrap();
        let old = addons.join(OLD_FOLDER);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("Translate.toc"), "## Title: Nai Ma Speak\n## Version: 0.3.0\n").unwrap();
        assert_eq!(addon_in(&addons), Addon::Installed(Some(Version([0, 3, 0]))));

        let zip = zipped(&[("NaiMaSpeak/NaiMaSpeak.toc", "## Version: 0.4.0\n")]);
        install_addon(&zip, &addons).unwrap();
        assert_eq!(addon_in(&addons), Addon::Installed(Some(Version([0, 4, 0]))));
        assert!(!old.exists(), "the old folder is gone");
        let settings = std::fs::read_to_string(saved.join("NaiMaSpeak.lua")).unwrap();
        assert_eq!(settings, "\nNaiMaSpeakDB = {\n[\"allLines\"] = true,\n}\n");

        // Another addon that happens to be named Translate is not ours to remove.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("Translate.toc"), "## Title: Translate\n## Version: 9.9.9\n").unwrap();
        install_addon(&zip, &addons).unwrap();
        assert!(old.join("Translate.toc").exists());
        remove(&addons.join(ADDON_FOLDER));
        assert_eq!(addon_in(&addons), Addon::Missing);
        remove(&game);
    }

    #[cfg(unix)]
    #[test]
    fn replaces_a_linked_addon_without_touching_what_it_led_to() {
        let addons = scratch("linked");
        let source = scratch("source");
        std::fs::write(source.join("NaiMaSpeak.toc"), "## Version: 0.1.0\n").unwrap();
        std::os::unix::fs::symlink(&source, addons.join(ADDON_FOLDER)).unwrap();
        assert_eq!(addon_in(&addons), Addon::Linked(Some(Version([0, 1, 0]))));
        // Asked for, the update takes the link's place and leaves what it led to alone.
        install_addon(&zipped(&[("NaiMaSpeak/NaiMaSpeak.toc", "## Version: 0.4.0\n")]), &addons).unwrap();
        assert_eq!(addon_in(&addons), Addon::Installed(Some(Version([0, 4, 0]))));
        assert_eq!(std::fs::read_to_string(source.join("NaiMaSpeak.toc")).unwrap(), "## Version: 0.1.0\n");
        remove(&addons);
        remove(&source);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn replaces_the_program() {
        let folder = scratch("program");
        let place = folder.join("nai-ma-speak");
        std::fs::write(&place, "old").unwrap();
        install_companion(b"new", &place).unwrap();
        assert_eq!(std::fs::read_to_string(&place).unwrap(), "new");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "nothing is left beside it");
        remove(&folder);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn replaces_the_app() {
        let folder = scratch("app");
        let place = folder.join("My Speak.app");
        std::fs::create_dir_all(place.join("Contents")).unwrap();
        std::fs::write(place.join("Contents/old"), "").unwrap();
        install_companion(&zipped(&[("Nai Ma Speak.app/Contents/new", "")]), &place).unwrap();
        assert!(place.join("Contents/new").exists() && !place.join("Contents/old").exists());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "nothing is left beside it");
        assert!(install_companion(&zipped(&[("readme.txt", "")]), &place).is_err());
        assert!(place.join("Contents/new").exists(), "a download with no app leaves the one installed");
        remove(&folder);
    }
}
