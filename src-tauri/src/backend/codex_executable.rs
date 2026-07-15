use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub fn resolve_codex_executable() -> PathBuf {
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")));
    resolve_codex_executable_from_roots(local_app_data.as_deref(), codex_home.as_deref())
}

fn resolve_codex_executable_from_roots(
    local_app_data: Option<&Path>,
    codex_home: Option<&Path>,
) -> PathBuf {
    if let Some(local_app_data) = local_app_data {
        let standalone_root = local_app_data
            .join("Programs")
            .join("OpenAI")
            .join("Codex")
            .join("bin");
        if let Some(candidate) =
            checked_candidate(&standalone_root.join("codex.exe"), &standalone_root)
        {
            return candidate.0;
        }
    }

    if let Some(codex_home) = codex_home {
        let standalone_root = codex_home
            .join("packages")
            .join("standalone")
            .join("current")
            .join("bin");
        if let Some(candidate) =
            checked_candidate(&standalone_root.join("codex.exe"), &standalone_root)
        {
            return candidate.0;
        }
    }

    resolve_codex_executable_from_local_app_data(local_app_data)
}

fn resolve_codex_executable_from_local_app_data(local_app_data: Option<&Path>) -> PathBuf {
    let Some(local_app_data) = local_app_data else {
        return fallback_codex_executable();
    };
    let bin_root = local_app_data.join("OpenAI").join("Codex").join("bin");
    let mut candidates = candidates_in_bin(&bin_root, &bin_root);

    let packages_root = local_app_data.join("Packages");
    if let Ok(packages) = fs::read_dir(&packages_root) {
        for package in packages.filter_map(Result::ok) {
            let package_name = package.file_name();
            if !package_name
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with("openai.codex_")
            {
                continue;
            }
            let package_bin = package
                .path()
                .join("LocalCache")
                .join("Local")
                .join("OpenAI")
                .join("Codex")
                .join("bin");
            candidates.extend(candidates_in_bin(&package_bin, &packages_root));
        }
    }

    candidates
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
        .map(|candidate| candidate.0)
        .unwrap_or_else(fallback_codex_executable)
}

fn candidates_in_bin(bin_root: &Path, allowed_root: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut candidates = Vec::new();
    if let Some(candidate) = checked_candidate(&bin_root.join("codex.exe"), allowed_root) {
        candidates.push(candidate);
    }
    if let Ok(entries) = fs::read_dir(bin_root) {
        candidates.extend(
            entries.filter_map(Result::ok).filter_map(|entry| {
                checked_candidate(&entry.path().join("codex.exe"), allowed_root)
            }),
        );
    }
    candidates
}

fn checked_candidate(path: &Path, allowed_root: &Path) -> Option<(PathBuf, SystemTime)> {
    let link_metadata = fs::symlink_metadata(path).ok()?;
    if !link_metadata.file_type().is_file() {
        return None;
    }
    let canonical_root = fs::canonicalize(allowed_root).ok()?;
    let canonical = fs::canonicalize(path).ok()?;
    if !canonical.starts_with(&canonical_root) {
        return None;
    }
    let metadata = fs::metadata(&canonical).ok()?;
    if !metadata.is_file() {
        return None;
    }
    Some((
        canonical,
        metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    ))
}

fn fallback_codex_executable() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from("codex.exe")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("codex")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fallback_codex_executable, resolve_codex_executable_from_local_app_data,
        resolve_codex_executable_from_roots,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "codex-orbit-executable-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("test directory should be created");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn add_candidate(&self, name: &str, contents: &[u8]) -> PathBuf {
            let candidate = self
                .path()
                .join("OpenAI")
                .join("Codex")
                .join("bin")
                .join(name)
                .join("codex.exe");
            fs::create_dir_all(candidate.parent().unwrap())
                .expect("candidate directory should be created");
            fs::write(&candidate, contents).expect("candidate should be written");
            candidate
        }

        fn add_file(&self, relative: impl AsRef<Path>) -> PathBuf {
            let candidate = self.path().join(relative);
            fs::create_dir_all(candidate.parent().unwrap())
                .expect("candidate directory should be created");
            fs::write(&candidate, b"candidate").expect("candidate should be written");
            candidate
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn selects_the_most_recent_valid_candidate() {
        let directory = TestDirectory::new("latest");
        let older = directory.add_candidate("older", b"older");
        std::thread::sleep(Duration::from_millis(20));
        let newer = directory.add_candidate("newer", b"newer");

        let selected = resolve_codex_executable_from_local_app_data(Some(directory.path()));

        assert_eq!(selected, fs::canonicalize(newer).unwrap());
        assert_ne!(selected, fs::canonicalize(older).unwrap());
    }

    #[test]
    fn prefers_the_official_programs_install_over_the_desktop_cache() {
        let directory = TestDirectory::new("programs");
        let cached = directory.add_candidate("cached", b"cached");
        let standalone = directory.add_file("Programs/OpenAI/Codex/bin/codex.exe");

        let selected = resolve_codex_executable_from_roots(Some(directory.path()), None);

        assert_eq!(selected, fs::canonicalize(standalone).unwrap());
        assert_ne!(selected, fs::canonicalize(cached).unwrap());
    }

    #[test]
    fn uses_the_codex_home_standalone_when_programs_is_missing() {
        let local_app_data = TestDirectory::new("local-app-data");
        let codex_home = TestDirectory::new("codex-home");
        let standalone = codex_home.add_file("packages/standalone/current/bin/codex.exe");

        let selected = resolve_codex_executable_from_roots(
            Some(local_app_data.path()),
            Some(codex_home.path()),
        );

        assert_eq!(selected, fs::canonicalize(standalone).unwrap());
    }

    #[test]
    fn discovers_a_flat_desktop_app_cache_binary() {
        let directory = TestDirectory::new("flat-desktop-cache");
        let cached = directory.add_file("OpenAI/Codex/bin/codex.exe");

        let selected = resolve_codex_executable_from_roots(Some(directory.path()), None);

        assert_eq!(selected, fs::canonicalize(cached).unwrap());
    }

    #[test]
    fn discovers_the_store_package_local_cache_binary() {
        let directory = TestDirectory::new("package-local-cache");
        let cached = directory
            .add_file("Packages/OpenAI.Codex_example/LocalCache/Local/OpenAI/Codex/bin/codex.exe");

        let selected = resolve_codex_executable_from_roots(Some(directory.path()), None);

        assert_eq!(selected, fs::canonicalize(cached).unwrap());
    }

    #[test]
    fn skips_directories_named_codex_exe() {
        let directory = TestDirectory::new("directory");
        let bin = directory.path().join("OpenAI").join("Codex").join("bin");
        fs::create_dir_all(bin.join("newest").join("codex.exe"))
            .expect("directory candidate should be created");
        let valid = directory.add_candidate("valid", b"valid");

        let selected = resolve_codex_executable_from_local_app_data(Some(directory.path()));

        assert_eq!(selected, fs::canonicalize(valid).unwrap());
    }

    #[test]
    fn falls_back_when_the_root_or_candidates_are_missing() {
        let directory = TestDirectory::new("fallback");

        assert_eq!(
            resolve_codex_executable_from_local_app_data(Some(directory.path())),
            fallback_codex_executable()
        );
        assert_eq!(
            resolve_codex_executable_from_local_app_data(None),
            fallback_codex_executable()
        );
    }

    #[cfg(windows)]
    #[test]
    fn rejects_a_candidate_whose_canonical_path_escapes_the_bin_root() {
        use std::os::windows::fs::symlink_file;

        let directory = TestDirectory::new("escape");
        let outside = directory.path().join("outside-codex.exe");
        fs::write(&outside, b"outside").unwrap();
        let link = directory
            .path()
            .join("OpenAI")
            .join("Codex")
            .join("bin")
            .join("escaped")
            .join("codex.exe");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        if symlink_file(&outside, &link).is_err() {
            return;
        }

        assert_eq!(
            resolve_codex_executable_from_local_app_data(Some(directory.path())),
            fallback_codex_executable()
        );
    }
}
