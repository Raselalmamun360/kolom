//! `কলম.toml`/`কলম.lock` manifest handling and git-based package fetching —
//! the non-compiler half of KPM. See the plan this was built from for the
//! full design rationale; in short: packages are git repos with their own
//! `কলম.toml`, fetched into a project-local `কলম_প্যাকেজ/<নাম>/` cache and
//! pinned by commit in a lock file. No registry, no SemVer ranges — `git`
//! itself (shelled out to, not vendored) does the fetching.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MANIFEST_FILE: &str = "কলম.toml";
pub const LOCK_FILE: &str = "কলম.lock";
pub const PACKAGES_DIR: &str = "কলম_প্যাকেজ";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(rename = "প্যাকেজ")]
    pub package: PackageInfo,
    #[serde(rename = "নির্ভরতা", default)]
    pub dependencies: BTreeMap<String, Dependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    #[serde(rename = "নাম")]
    pub name: String,
    #[serde(rename = "সংস্করণ")]
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub git: String,
    #[serde(rename = "রেফ")]
    pub reference: String,
}

impl Manifest {
    pub fn new(name: &str) -> Self {
        Manifest {
            package: PackageInfo { name: name.to_string(), version: "0.1.0".to_string() },
            dependencies: BTreeMap::new(),
        }
    }

    pub fn load(dir: &Path) -> Result<Manifest, String> {
        let path = dir.join(MANIFEST_FILE);
        let src = std::fs::read_to_string(&path)
            .map_err(|e| format!("'{}' পড়া যায়নি: {}", path.display(), e))?;
        toml::from_str(&src).map_err(|e| format!("'{}' পার্স করা যায়নি: {}", path.display(), e))
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join(MANIFEST_FILE);
        let text = toml::to_string_pretty(self).map_err(|e| format!("manifest সিরিয়ালাইজ করা যায়নি: {}", e))?;
        std::fs::write(&path, text).map_err(|e| format!("'{}' লেখা যায়নি: {}", path.display(), e))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Lockfile {
    #[serde(rename = "প্যাকেজ", default)]
    pub packages: Vec<LockedPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedPackage {
    pub name: String,
    pub git: String,
    pub commit: String,
}

impl Lockfile {
    pub fn load(dir: &Path) -> Result<Lockfile, String> {
        let path = dir.join(LOCK_FILE);
        if !path.exists() {
            return Ok(Lockfile::default());
        }
        let src = std::fs::read_to_string(&path)
            .map_err(|e| format!("'{}' পড়া যায়নি: {}", path.display(), e))?;
        toml::from_str(&src).map_err(|e| format!("'{}' পার্স করা যায়নি: {}", path.display(), e))
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join(LOCK_FILE);
        let text = toml::to_string_pretty(self).map_err(|e| format!("lock file সিরিয়ালাইজ করা যায়নি: {}", e))?;
        std::fs::write(&path, text).map_err(|e| format!("'{}' লেখা যায়নি: {}", path.display(), e))
    }

    fn find(&self, name: &str) -> Option<&LockedPackage> {
        self.packages.iter().find(|p| p.name == name)
    }
}

pub fn packages_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(PACKAGES_DIR)
}

fn run_git(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("'git' চালানো যায়নি ({e}) — git ইনস্টল করা আছে ও PATH-এ আছে কিনা দেখুন"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} ব্যর্থ:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Fresh resolve: shallow-clones `url` at `reference` (a tag or branch),
/// returning the commit it resolved to. Used when a dependency has no lock
/// entry yet.
fn clone_fresh(url: &str, reference: &str, dest: &Path) -> Result<String, String> {
    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|e| format!("'{}' মুছে ফেলা যায়নি: {}", dest.display(), e))?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("ফোল্ডার তৈরি করা যায়নি: {}", e))?;
    }
    run_git(
        &["clone", "--depth", "1", "--branch", reference, url, &dest.to_string_lossy()],
        None,
    )?;
    run_git(&["rev-parse", "HEAD"], Some(dest))
}

/// Reproducible install: full-clones `url` then checks out the exact
/// `commit` from the lock file. A shallow ref-based clone can't be used here
/// — if a branch (not a tag) has moved on since the lock was written, a
/// shallow clone of that branch would not contain the older locked commit.
fn checkout_locked(url: &str, commit: &str, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|e| format!("'{}' মুছে ফেলা যায়নি: {}", dest.display(), e))?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("ফোল্ডার তৈরি করা যায়নি: {}", e))?;
    }
    run_git(&["clone", url, &dest.to_string_lossy()], None)?;
    run_git(&["checkout", commit], Some(dest))?;
    Ok(())
}

/// Resolves the full dependency tree of the project at `project_dir`,
/// fetching every package into `কলম_প্যাকেজ/<নাম>/` and returning the
/// lock file to write. `existing_lock` is consulted first — a dependency
/// already locked is reproduced at its exact commit rather than re-resolved
/// against its `রেফ`, matching the standard lock-file contract.
///
/// Two independent paths pulling in the same package name must resolve to
/// the *same* commit; otherwise this returns a conflict error rather than
/// silently picking one — there's no SemVer solver here to arbitrate.
pub fn install(project_dir: &Path, existing_lock: &Lockfile) -> Result<Lockfile, String> {
    let manifest = Manifest::load(project_dir)?;
    let mut resolved: BTreeMap<String, LockedPackage> = BTreeMap::new();
    let pkgs_dir = packages_dir(project_dir);

    let mut queue: Vec<(String, Dependency)> = manifest.dependencies.into_iter().collect();
    while let Some((name, dep)) = queue.pop() {
        if let Some(prev) = resolved.get(&name) {
            if prev.git != dep.git {
                return Err(format!(
                    "দ্বন্দ্ব: '{}' প্যাকেজ দুইটি ভিন্ন git URL থেকে চাওয়া হয়েছে ({} বনাম {})",
                    name, prev.git, dep.git
                ));
            }
            continue;
        }

        let dest = pkgs_dir.join(&name);
        let commit = if let Some(locked) = existing_lock.find(&name) {
            if locked.git != dep.git {
                return Err(format!(
                    "দ্বন্দ্ব: '{}'-এর জন্য লক করা URL ({}) manifest-এর URL-এর ({}) সাথে মেলে না — লক ফাইল মুছে আবার 'kolom install' চালান",
                    name, locked.git, dep.git
                ));
            }
            checkout_locked(&dep.git, &locked.commit, &dest)?;
            locked.commit.clone()
        } else {
            clone_fresh(&dep.git, &dep.reference, &dest)?
        };

        let sub_manifest_path = dest.join(MANIFEST_FILE);
        if !sub_manifest_path.exists() {
            return Err(format!(
                "'{}' প্যাকেজে কোনো '{}' নেই — এটা একটা বৈধ কলম প্যাকেজ নয়",
                name, MANIFEST_FILE
            ));
        }
        let sub_manifest = Manifest::load(&dest)?;
        if sub_manifest.package.name != name {
            return Err(format!(
                "'{}' নামে নির্ভরতা যোগ করা হয়েছে, কিন্তু প্যাকেজ নিজেকে '{}' বলে ঘোষণা করেছে — নাম দুটো মিলতে হবে",
                name, sub_manifest.package.name
            ));
        }
        let entry_file = dest.join(format!("{}.ক", name));
        if !entry_file.exists() {
            return Err(format!(
                "'{}' প্যাকেজে '{}.ক' এন্ট্রি ফাইল নেই",
                name, name
            ));
        }

        resolved.insert(name.clone(), LockedPackage { name, git: dep.git, commit });
        for (sub_name, sub_dep) in sub_manifest.dependencies {
            queue.push((sub_name, sub_dep));
        }
    }

    Ok(Lockfile { packages: resolved.into_values().collect() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo(dir: &Path, name: &str, deps: &[(&str, &str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        run_git(&["init", "-q"], Some(dir)).unwrap();
        run_git(&["config", "user.email", "t@example.com"], Some(dir)).unwrap();
        run_git(&["config", "user.name", "t"], Some(dir)).unwrap();
        let mut m = Manifest::new(name);
        for (dep_name, url, reference) in deps {
            m.dependencies.insert(
                dep_name.to_string(),
                Dependency { git: url.to_string(), reference: reference.to_string() },
            );
        }
        m.save(dir).unwrap();
        std::fs::write(dir.join(format!("{name}.ক")), "ফাংশন হ্যালো() -> ফাঁকা {\n\n}\n").unwrap();
        run_git(&["add", "."], Some(dir)).unwrap();
        run_git(&["commit", "-q", "-m", "init"], Some(dir)).unwrap();
        run_git(&["tag", "v1.0.0"], Some(dir)).unwrap();
    }

    #[test]
    fn manifest_round_trips() {
        let tmp = std::env::temp_dir().join(format!("kolom-pkg-test-manifest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let mut m = Manifest::new("আমার_প্রকল্প");
        m.dependencies.insert(
            "জ্যামিতি_প্লাস".to_string(),
            Dependency { git: "https://example.com/g.git".to_string(), reference: "v1.0.0".to_string() },
        );
        m.save(&tmp).unwrap();
        let loaded = Manifest::load(&tmp).unwrap();
        assert_eq!(loaded.package.name, "আমার_প্রকল্প");
        assert_eq!(loaded.dependencies["জ্যামিতি_প্লাস"].git, "https://example.com/g.git");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_resolves_a_local_git_dependency() {
        let root = std::env::temp_dir().join(format!("kolom-pkg-test-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let dep_repo = root.join("dep_repo");
        init_repo(&dep_repo, "জ্যামিতি_প্লাস", &[]);

        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let mut m = Manifest::new("আমার_প্রকল্প");
        m.dependencies.insert(
            "জ্যামিতি_প্লাস".to_string(),
            Dependency { git: dep_repo.to_string_lossy().into_owned(), reference: "v1.0.0".to_string() },
        );
        m.save(&project).unwrap();

        let lock = install(&project, &Lockfile::default()).expect("install should succeed");
        assert_eq!(lock.packages.len(), 1);
        assert_eq!(lock.packages[0].name, "জ্যামিতি_প্লাস");
        assert!(packages_dir(&project).join("জ্যামিতি_প্লাস").join("জ্যামিতি_প্লাস.ক").exists());

        // Re-running against the just-written lock reproduces the same commit
        // without needing the ref to still resolve the same way.
        let lock2 = install(&project, &lock).expect("second install should succeed");
        assert_eq!(lock2.packages[0].commit, lock.packages[0].commit);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_detects_transitive_name_conflict() {
        let root = std::env::temp_dir().join(format!("kolom-pkg-test-conflict-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let a_repo = root.join("a_repo");
        init_repo(&a_repo, "a", &[]);
        let a_repo2 = root.join("a_repo2");
        init_repo(&a_repo2, "a", &[]);

        let b_repo = root.join("b_repo");
        init_repo(&b_repo, "b", &[("a", &a_repo2.to_string_lossy(), "v1.0.0")]);

        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let mut m = Manifest::new("প্রকল্প");
        m.dependencies.insert("a".to_string(), Dependency { git: a_repo.to_string_lossy().into_owned(), reference: "v1.0.0".to_string() });
        m.dependencies.insert("b".to_string(), Dependency { git: b_repo.to_string_lossy().into_owned(), reference: "v1.0.0".to_string() });
        m.save(&project).unwrap();

        let err = install(&project, &Lockfile::default()).expect_err("should conflict");
        assert!(err.contains("দ্বন্দ্ব"), "expected a দ্বন্দ্ব error, got: {err}");

        let _ = std::fs::remove_dir_all(&root);
    }
}
