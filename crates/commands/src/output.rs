//! Confined sidecar output. On Unix every operation is relative to a pinned
//! directory descriptor; replacing a directory path cannot redirect a write.

use std::io::{self, Write};
#[cfg(unix)]
use std::path::Component;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A pinned destination directory; all publication stays relative to it.
struct Directory {
    #[cfg(unix)]
    fd: rustix::fd::OwnedFd,
    #[cfg(not(unix))]
    path: PathBuf,
    // Windows share modes pin each ancestor's name while outputs are staged
    // and published. Opening the final directory alone would leave its parents
    // replaceable with junctions/reparse points during a path-based write.
    #[cfg(windows)]
    _ancestors: Vec<std::fs::File>,
}

impl Directory {
    /// Pin a contained output directory before any file is staged.
    fn open(root: &Path, parent: &Path) -> io::Result<Self> {
        let root = root.canonicalize()?;
        let parent = parent.canonicalize()?;
        let relative = parent.strip_prefix(&root).map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "output is outside the library",
            )
        })?;
        #[cfg(unix)]
        {
            use rustix::fs::{open, openat, Mode, OFlags};
            let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let mut fd = open(&root, flags, Mode::empty())?;
            for part in relative.components() {
                let Component::Normal(name) = part else {
                    return Err(io::Error::other("invalid output parent"));
                };
                fd = openat(&fd, name, flags, Mode::empty())?;
            }
            Ok(Self { fd })
        }
        #[cfg(not(unix))]
        {
            let _ = relative;
            #[cfg(windows)]
            let ancestors = pin_windows_ancestors(&parent)?;
            Ok(Self {
                path: parent,
                #[cfg(windows)]
                _ancestors: ancestors,
            })
        }
    }

    /// Check an entry without following links; distinguish absent from unsafe.
    fn regular(&self, name: &std::ffi::OsStr) -> io::Result<bool> {
        #[cfg(unix)]
        {
            use rustix::fs::{statat, AtFlags, FileType};
            match statat(&self.fd, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(s) if FileType::from_raw_mode(s.st_mode) == FileType::RegularFile => Ok(true),
                Ok(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "output target is not a regular file",
                )),
                Err(rustix::io::Errno::NOENT) => Ok(false),
                Err(e) => Err(e.into()),
            }
        }
        #[cfg(not(unix))]
        {
            match std::fs::symlink_metadata(self.path.join(name)) {
                Ok(m) if m.is_file() && !m.file_type().is_symlink() => Ok(true),
                Ok(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "output target is not a regular file",
                )),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e),
            }
        }
    }
    #[cfg(unix)]
    /// Read replacement permissions without following a target link.
    fn permissions(&self, name: &std::ffi::OsStr) -> io::Result<Option<std::fs::Permissions>> {
        use std::os::unix::fs::PermissionsExt;
        match rustix::fs::statat(&self.fd, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat)
                if rustix::fs::FileType::from_raw_mode(stat.st_mode)
                    == rustix::fs::FileType::RegularFile =>
            {
                // mode_t is u16 on Darwin and u32 on Linux. Keep the widening
                // conversion even where it is an identity conversion.
                #[allow(clippy::useless_conversion)]
                let mode = u32::from(stat.st_mode & 0o777);
                Ok(Some(std::fs::Permissions::from_mode(mode)))
            }
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "output target is not a regular file",
            )),
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Reserve a unique staging file in this pinned directory.
    fn temporary(&self) -> io::Result<(std::ffi::OsString, std::fs::File)> {
        for _ in 0..100 {
            let name = format!(
                ".tagrex-output-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            #[cfg(unix)]
            let file = rustix::fs::openat(
                &self.fd,
                name.as_str(),
                rustix::fs::OFlags::WRONLY
                    | rustix::fs::OFlags::CREATE
                    | rustix::fs::OFlags::EXCL
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::RUSR
                    | rustix::fs::Mode::WUSR
                    | rustix::fs::Mode::RGRP
                    | rustix::fs::Mode::WGRP
                    | rustix::fs::Mode::ROTH
                    | rustix::fs::Mode::WOTH,
            )
            .map(std::fs::File::from)
            .map_err(io::Error::from);
            #[cfg(not(unix))]
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.path.join(&name));
            match file {
                Ok(file) => return Ok((name.into(), file)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve output file",
        ))
    }

    /// Move a staging or recovery name within the pinned directory.
    fn rename(&self, from: &std::ffi::OsStr, to: &std::ffi::OsStr) -> io::Result<()> {
        #[cfg(unix)]
        {
            Ok(rustix::fs::renameat(&self.fd, from, &self.fd, to)?)
        }
        #[cfg(not(unix))]
        {
            std::fs::rename(self.path.join(from), self.path.join(to))
        }
    }

    /// Publish without replacing a racing target when supported by the filesystem.
    fn publish(&self, from: &std::ffi::OsStr, to: &std::ffi::OsStr) -> io::Result<()> {
        #[cfg(unix)]
        {
            publish_with_fallback(
                || {
                    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
                    return rustix::fs::renameat_with(
                        &self.fd,
                        from,
                        &self.fd,
                        to,
                        rustix::fs::RenameFlags::NOREPLACE,
                    );
                    #[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
                    Err(rustix::io::Errno::NOSYS)
                },
                || rustix::fs::linkat(&self.fd, from, &self.fd, to, rustix::fs::AtFlags::empty()),
                || self.publish_by_checked_rename(from, to),
            )
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
            let wide = |name: &std::ffi::OsStr| -> io::Result<Vec<u16>> {
                let mut path: Vec<u16> = self.path.join(name).as_os_str().encode_wide().collect();
                if path.contains(&0) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "NUL in output path",
                    ));
                }
                path.push(0);
                Ok(path)
            };
            let from = wide(from)?;
            let to = wide(to)?;
            // No REPLACE_EXISTING flag: a target appearing after staging wins.
            // Unlike hard links this also works on FAT/exFAT. Both names are
            // under the same pinned directory, so no cross-volume copy occurs.
            // https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw
            // SAFETY: both buffers are NUL-terminated and live for this call;
            // the API borrows the pointers and does not retain them.
            if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
        #[cfg(not(any(unix, windows)))]
        {
            std::fs::hard_link(self.path.join(from), self.path.join(to))
        }
    }

    /// Compatibility path for filesystems without atomic no-replace rename or
    /// hard links. There remains a race between the absence check and rename;
    /// it is confined to this pinned directory and never follows target links.
    #[cfg(unix)]
    fn publish_by_checked_rename(
        &self,
        from: &std::ffi::OsStr,
        to: &std::ffi::OsStr,
    ) -> io::Result<()> {
        if self.regular(to)? {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        self.rename(from, to)
    }

    /// Best-effort cleanup confined to this directory.
    fn remove(&self, name: &std::ffi::OsStr) {
        #[cfg(unix)]
        {
            let _ = rustix::fs::unlinkat(&self.fd, name, rustix::fs::AtFlags::empty());
        }
        #[cfg(not(unix))]
        {
            let _ = std::fs::remove_file(self.path.join(name));
        }
    }
}

/// Prefer atomic no-replace operations; only unsupported-operation errors may
/// reach the final checked rename. Permission, collision and I/O errors stop.
#[cfg(unix)]
fn publish_with_fallback(
    exclusive_rename: impl FnOnce() -> rustix::io::Result<()>,
    hard_link: impl FnOnce() -> rustix::io::Result<()>,
    checked_rename: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let unsupported = |e: rustix::io::Errno| {
        // ENOTSUP and EOPNOTSUPP are distinct on Darwin, aliases on Linux.
        e == rustix::io::Errno::NOSYS
            || e == rustix::io::Errno::INVAL
            || e == rustix::io::Errno::NOTSUP
            || e == rustix::io::Errno::OPNOTSUPP
    };
    match exclusive_rename() {
        Ok(()) => return Ok(()),
        Err(e) if unsupported(e) => {}
        Err(e) => return Err(e.into()),
    }
    match hard_link() {
        Ok(()) => Ok(()),
        Err(e) if unsupported(e) => checked_rename(),
        Err(e) => Err(e.into()),
    }
}

/// Windows name-surrogate reparse tags redirect a path; cloud placeholders do not.
/// https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-point-tags
#[cfg(any(windows, test))]
fn name_surrogate_tag(tag: u32) -> bool {
    tag & 0x2000_0000 != 0
}

/// Pin every ancestor against rename without requiring directory-list access.
#[cfg(windows)]
fn pin_windows_ancestors(parent: &Path) -> io::Result<Vec<std::fs::File>> {
    use std::os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FileAttributeTagInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_TAG_INFO,
    };
    // Keep ownership in std::fs::File; only the reparse-tag query borrows its
    // raw handle for one Windows SDK call.
    // Metadata-only opens do not participate in Windows sharing checks. Ask
    // for traverse/execute access too, so omitting FILE_SHARE_DELETE actually
    // prevents renames until these handles are dropped.
    // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/8c0e3f4f-0729-49f4-a14d-7f7add593819
    // FILE_TRAVERSE shares the FILE_EXECUTE bit, which participates in the
    // documented sharing check but does not require listing ancestor contents.
    const FILE_TRAVERSE: u32 = 0x20;
    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    const FILE_SHARE_READ_WRITE: u32 = 0x1 | 0x2;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let mut handles = Vec::new();
    let mut path = PathBuf::new();
    for component in parent.components() {
        path.push(component.as_os_str());
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .access_mode(FILE_TRAVERSE | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)?;
        let metadata = handle.metadata()?;
        if !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "output ancestor is not a plain directory",
            ));
        }
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            let mut info = FILE_ATTRIBUTE_TAG_INFO {
                FileAttributes: 0,
                ReparseTag: 0,
            };
            // Query the pinned handle, not a second path lookup. Cloud-file tags
            // do not redirect names; symlinks and junctions have the surrogate bit.
            // SAFETY: the live handle is borrowed; info is writable for exactly
            // its declared size and the API retains neither pointer.
            if unsafe {
                GetFileInformationByHandleEx(
                    handle.as_raw_handle(),
                    FileAttributeTagInfo,
                    (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                    std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if name_surrogate_tag(info.ReparseTag) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "output ancestor redirects its name",
                ));
            }
        }
        handles.push(handle);
    }
    Ok(handles)
}

struct Staged {
    dir: Directory,
    name: std::ffi::OsString,
    temporary: std::ffi::OsString,
    backup: Option<std::ffi::OsString>,
    published: bool,
}

impl Drop for Staged {
    fn drop(&mut self) {
        self.dir.remove(&self.temporary);
    }
}

/// Stage the entire batch before publishing. Existing links (including dangling
/// ones) and special files are rejected. A later failure restores prior outputs.
pub(crate) fn write_batch(
    root: &Path,
    outputs: &[(PathBuf, Vec<u8>)],
    overwrite: bool,
) -> io::Result<()> {
    let mut staged = Vec::new();
    for (target, bytes) in outputs {
        let parent = target
            .parent()
            .ok_or_else(|| io::Error::other("missing output parent"))?;
        let name = target
            .file_name()
            .ok_or_else(|| io::Error::other("missing output name"))?
            .to_owned();
        let dir = Directory::open(root, parent)?;
        if dir.regular(&name)? && !overwrite {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "output already exists",
            ));
        }
        let (temporary, mut file) = dir.temporary()?;
        let entry = Staged {
            dir,
            name,
            temporary,
            backup: None,
            published: false,
        };
        let result = (|| {
            #[cfg(unix)]
            if let Some(permissions) = entry.dir.permissions(&entry.name)? {
                file.set_permissions(permissions)?;
            }
            file.write_all(bytes)?;
            file.sync_all()
        })();
        drop(file);
        result?;
        staged.push(entry);
    }
    let result = (|| {
        for entry in &mut staged {
            if entry.dir.regular(&entry.name)? {
                if !overwrite {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "output already exists",
                    ));
                }
                let (backup, file) = entry.dir.temporary()?;
                drop(file);
                if let Err(e) = entry.dir.rename(&entry.name, &backup) {
                    entry.dir.remove(&backup);
                    return Err(e);
                }
                entry.backup = Some(backup);
                // A target switched to a link after the check is moved, never
                // opened. Validate the moved object before replacing it.
                entry.dir.regular(entry.backup.as_deref().unwrap())?;
            }
            entry.dir.publish(&entry.temporary, &entry.name)?;
            entry.published = true;
        }
        Ok(())
    })();
    if let Err(cause) = &result {
        let mut failures = Vec::new();
        for entry in staged.iter().rev() {
            if entry.published {
                entry.dir.remove(&entry.name);
            }
            if let Some(backup) = &entry.backup {
                if let Err(error) = entry.dir.rename(backup, &entry.name) {
                    failures.push(format!(
                        "{} (backup {}): {error}",
                        entry.name.to_string_lossy(),
                        backup.to_string_lossy()
                    ));
                }
            }
        }
        if !failures.is_empty() {
            return Err(io::Error::other(format!(
                "{}; output restoration failed; originals retained in {}",
                cause,
                failures.join(", ")
            )));
        }
    } else {
        for entry in &staged {
            if let Some(backup) = &entry.backup {
                entry.dir.remove(backup);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root(label: &str) -> PathBuf {
        let base = std::env::var_os("TAGREX_OUTPUT_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let p = base.join(format!(
            "tagrex-output-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    // macOS may create AppleDouble resource-fork companions on FAT/exFAT.
    fn output_entries(root: &Path) -> usize {
        std::fs::read_dir(root)
            .unwrap()
            .filter(|entry| {
                !entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("._")
            })
            .count()
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_atomic_publication_reaches_checked_rename() {
        let root = root("fallback");
        let dir = Directory::open(&root, &root).unwrap();
        let (temporary, mut file) = dir.temporary().unwrap();
        file.write_all(b"fallback").unwrap();
        drop(file);
        let name = std::ffi::OsStr::new("cover.jpg");
        publish_with_fallback(
            || Err(rustix::io::Errno::NOTSUP),
            || Err(rustix::io::Errno::OPNOTSUPP),
            || dir.publish_by_checked_rename(&temporary, name),
        )
        .unwrap();
        assert_eq!(std::fs::read(root.join(name)).unwrap(), b"fallback");
        // Existing files and links must still stop the final fallback.
        let (second, mut file) = dir.temporary().unwrap();
        file.write_all(b"must not replace").unwrap();
        drop(file);
        assert_eq!(
            dir.publish_by_checked_rename(&second, name)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(root.join(name)).unwrap(), b"fallback");
        dir.remove(&second);
        drop(dir);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn checked_rename_refuses_existing_and_dangling_links() {
        let root = root("fallback-links");
        let dir = Directory::open(&root, &root).unwrap();
        let (temporary, file) = dir.temporary().unwrap();
        drop(file);
        std::fs::write(root.join("original"), b"untouched").unwrap();
        for (name, target) in [("live", "original"), ("dangling", "missing")] {
            std::os::unix::fs::symlink(target, root.join(name)).unwrap();
            assert!(dir
                .publish_by_checked_rename(&temporary, std::ffi::OsStr::new(name))
                .is_err());
            assert!(std::fs::symlink_metadata(root.join(name))
                .unwrap()
                .file_type()
                .is_symlink());
        }
        assert_eq!(std::fs::read(root.join("original")).unwrap(), b"untouched");
        dir.remove(&temporary);
        drop(dir);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn publication_fallback_does_not_hide_collisions_or_permission_errors() {
        for error in [
            rustix::io::Errno::EXIST,
            rustix::io::Errno::ACCESS,
            rustix::io::Errno::IO,
        ] {
            let result = publish_with_fallback(
                || Err(error),
                || panic!("must stop"),
                || panic!("must stop"),
            );
            assert_eq!(
                result.unwrap_err().raw_os_error(),
                Some(error.raw_os_error())
            );
            let result = publish_with_fallback(
                || Err(rustix::io::Errno::NOTSUP),
                || Err(error),
                || panic!("must stop"),
            );
            assert_eq!(
                result.unwrap_err().raw_os_error(),
                Some(error.raw_os_error())
            );
        }
        publish_with_fallback(
            || Ok(()),
            || panic!("rename succeeded"),
            || panic!("rename succeeded"),
        )
        .unwrap();
        publish_with_fallback(
            || Err(rustix::io::Errno::NOTSUP),
            || Ok(()),
            || panic!("link succeeded"),
        )
        .unwrap();
    }

    #[test]
    fn publishes_new_files_and_replaces_existing_files_without_leftovers() {
        let root = root("publish");
        let target = root.join("cover.jpg");
        write_batch(&root, &[(target.clone(), b"first".to_vec())], false).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"first");
        assert!(write_batch(&root, &[(target.clone(), b"refused".to_vec())], false).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"first");
        write_batch(&root, &[(target.clone(), b"replacement".to_vec())], true).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"replacement");
        assert_eq!(output_entries(&root), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn publication_preserves_a_target_that_appears_after_staging() {
        let root = root("racing-target");
        let dir = Directory::open(&root, &root).unwrap();
        let (temporary, mut file) = dir.temporary().unwrap();
        file.write_all(b"staged").unwrap();
        drop(file);
        let target = std::ffi::OsStr::new("cover.jpg");
        std::fs::write(root.join(target), b"racing writer").unwrap();
        assert!(dir.publish(&temporary, target).is_err());
        assert_eq!(std::fs::read(root.join(target)).unwrap(), b"racing writer");
        dir.remove(&temporary);
        drop(dir);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_name_surrogate_tags_redirect_windows_paths() {
        assert!(name_surrogate_tag(0xa000_000c)); // symlink
        assert!(name_surrogate_tag(0xa000_0003)); // junction/mount point
        assert!(!name_surrogate_tag(0x9000_001a)); // cloud
        assert!(!name_surrogate_tag(0x9000_f01a)); // cloud F
        assert!(!name_surrogate_tag(0)); // ordinary directory
    }

    #[cfg(windows)]
    #[test]
    fn traverse_only_parent_can_be_pinned_without_listing() {
        let root = root("traverse-only");
        let child = root.join("child");
        std::fs::create_dir(&child).unwrap();
        let script = r#"
            $ErrorActionPreference = 'Stop'
            $acl = Get-Acl -LiteralPath $env:TAGREX_PIN_TEST_PARENT
            $user = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
            $rule = [System.Security.AccessControl.FileSystemAccessRule]::new($user, [System.Security.AccessControl.FileSystemRights]::ListDirectory, [System.Security.AccessControl.AccessControlType]::Deny)
            $acl.AddAccessRule($rule)
            Set-Acl -LiteralPath $env:TAGREX_PIN_TEST_PARENT -AclObject $acl
        "#;
        let status = std::process::Command::new("powershell.exe")
            // Cargo may itself run under PowerShell 7, whose module paths
            // cannot be loaded by the Windows PowerShell 5.1 child.
            .env_remove("PSModulePath")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("TAGREX_PIN_TEST_PARENT", &root)
            .status()
            .unwrap();
        assert!(status.success());
        let result = (|| {
            assert!(std::fs::read_dir(&root).is_err());
            let pinned = Directory::open(&root, &child)?;
            let (name, file) = pinned.temporary()?;
            drop(file);
            pinned.publish(&name, std::ffi::OsStr::new("cover.jpg"))?;
            Ok::<_, io::Error>(())
        })();
        // Restore only this test directory's inherited ACL before cleanup.
        let status = std::process::Command::new("icacls.exe")
            .arg(&root)
            .args(["/reset", "/Q"])
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::remove_dir_all(root).unwrap();
        result.unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn pinned_windows_ancestors_cannot_be_renamed_until_publication_finishes() {
        let root = root("windows-pins");
        let album = root.join("album");
        let disc = album.join("disc");
        std::fs::create_dir_all(&disc).unwrap();
        let pinned = Directory::open(&root, &disc).unwrap();
        assert!(std::fs::rename(&disc, album.join("replacement-disc")).is_err());
        assert!(std::fs::rename(&album, root.join("replacement")).is_err());
        let moved_root = root.with_extension("moved");
        assert!(std::fs::rename(&root, &moved_root).is_err());

        // Pinning names must not prevent publishing files beneath them.
        let (temporary, mut file) = pinned.temporary().unwrap();
        file.write_all(b"cover").unwrap();
        drop(file);
        pinned
            .publish(&temporary, std::ffi::OsStr::new("cover.jpg"))
            .unwrap();
        pinned.remove(&temporary);
        drop(pinned);

        std::fs::rename(&disc, album.join("replacement-disc")).unwrap();
        std::fs::rename(&album, root.join("replacement")).unwrap();
        std::fs::rename(&root, &moved_root).unwrap();
        assert_eq!(
            std::fs::read(moved_root.join("replacement/replacement-disc/cover.jpg")).unwrap(),
            b"cover"
        );
        std::fs::remove_dir_all(moved_root).unwrap();
    }
    #[test]
    fn a_later_invalid_target_leaves_the_entire_batch_untouched() {
        let root = root("batch");
        std::fs::write(root.join("first.jpg"), b"original").unwrap();
        std::fs::create_dir(root.join("second.jpg")).unwrap();
        assert!(write_batch(
            &root,
            &[
                (root.join("first.jpg"), b"changed".to_vec()),
                (root.join("second.jpg"), vec![1])
            ],
            true
        )
        .is_err());
        assert_eq!(std::fs::read(root.join("first.jpg")).unwrap(), b"original");
        assert_eq!(output_entries(&root), 2);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn pinned_parent_cannot_be_redirected_by_a_directory_symlink() {
        let root = root("parent-race");
        let inner = root.join("album");
        let outside = root.join("outside");
        std::fs::create_dir(&inner).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let dir = Directory::open(&root, &inner).unwrap();
        std::fs::rename(&inner, root.join("original-album")).unwrap();
        std::os::unix::fs::symlink(&outside, &inner).unwrap();
        let (temp, mut f) = dir.temporary().unwrap();
        f.write_all(b"image").unwrap();
        drop(f);
        dir.publish(&temp, std::ffi::OsStr::new("cover.jpg"))
            .unwrap();
        dir.remove(&temp);
        assert!(!outside.join("cover.jpg").exists());
        assert_eq!(
            std::fs::read(root.join("original-album/cover.jpg")).unwrap(),
            b"image"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
