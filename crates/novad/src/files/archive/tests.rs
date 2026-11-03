//! Archives written as other tools write them (zip, tar and its
//! compressions), then listed, extracted and packed through this module.

use super::*;
use nova_test_support::TempDir;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

/// A zip of `members`: a name ending in `/` is a folder, `->` makes a link.
pub(crate) fn write_zip(path: &Path, members: &[(&str, &str)]) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let options = SimpleFileOptions::default();
    for (name, contents) in members {
        if let Some(target) = contents.strip_prefix("-> ") {
            zip.add_symlink(*name, target, options).unwrap();
        } else if name.ends_with('/') {
            zip.add_directory(*name, options).unwrap();
        } else {
            zip.start_file(*name, options.unix_permissions(0o4755)).unwrap();
            zip.write_all(contents.as_bytes()).unwrap();
        }
    }
    zip.finish().unwrap();
}

/// A tar of `members`, as [`write_zip`] takes them, through `compress`.
fn write_tar<W: Write>(out: W, members: &[(&str, &str)]) -> W {
    let mut tar = tar::Builder::new(out);
    for (name, contents) in members {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_mtime(1_700_000_000);
        if let Some(target) = contents.strip_prefix("-> ") {
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            tar.append_link(&mut header, name, target).unwrap();
        } else if name.ends_with('/') {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            tar.append_data(&mut header, name, std::io::empty()).unwrap();
        } else {
            header.set_size(contents.len() as u64);
            tar.append_data(&mut header, name, contents.as_bytes()).unwrap();
        }
    }
    tar.into_inner().unwrap()
}

fn names(entries: &[FileEntry]) -> Vec<(String, bool)> {
    let mut names: Vec<_> = entries.iter().map(|entry| (entry.name.clone(), entry.dir)).collect();
    names.sort();
    names
}

fn listed(archive: &Path, inner: &str, hidden: bool) -> io::Result<Vec<FileEntry>> {
    let inside = Inside {
        archive: archive.to_owned(),
        kind: Kind::of_name(archive.file_name().unwrap()).unwrap(),
        inner: inner.into(),
    };
    list(&inside, hidden, &MimeDb::default(), &|| false).map(Option::unwrap)
}

fn unpacked(archive: &Path, into: &Path) {
    fs::create_dir_all(into).unwrap();
    let kind = Kind::of_name(archive.file_name().unwrap()).unwrap();
    assert!(unpack(archive, kind, into, &mut |_| true).unwrap());
}

#[test]
fn archives_are_known_by_name_and_extract_to_their_stem() {
    for (name, kind) in
        [("a.zip", Kind::Zip), ("A.TAR.GZ", Kind::TarGz), ("b.tgz", Kind::TarGz), ("c.tar.zst", Kind::TarZst)]
    {
        assert_eq!(Kind::of_name(OsStr::new(name)), Some(kind), "{name}");
    }
    assert_eq!(Kind::of_name(OsStr::new(".zip")), None, "only an ending");
    assert_eq!(Kind::of_name(OsStr::new("notes.gz")), None, "a gzip of one file isn't a folder");
    assert_eq!(folder_name(OsStr::new("Photos 2025.tar.gz")), "Photos 2025");
}

#[test]
fn a_path_through_a_zip_is_inside_it_and_a_folder_named_like_one_isnt() {
    let dir = TempDir::new("archive-inside");
    write_zip(&dir.join("photos.zip"), &[("2025/a.png", "png")]);
    fs::create_dir_all(dir.join("backup.zip/inner")).unwrap();
    let inside = Inside::of(&dir.join("photos.zip/2025")).unwrap();
    assert_eq!((inside.archive, inside.kind, inside.inner), (dir.join("photos.zip"), Kind::Zip, "2025".into()));
    assert_eq!(Inside::of(&dir.join("photos.zip")).unwrap().inner, PathBuf::new());
    assert_eq!(Inside::of(&dir.join("backup.zip/inner")), None);
}

#[test]
fn a_zip_lists_like_a_folder_with_the_folders_it_only_implies() {
    let dir = TempDir::new("archive-list");
    let zip = dir.join("photos.zip");
    write_zip(
        &zip,
        &[("cover.png", "png"), ("2025/a.png", "1234"), ("2025/trip/b.png", "b"), (".hidden", "h"), ("empty/", "")],
    );
    assert_eq!(
        names(&listed(&zip, "", false).unwrap()),
        [("2025".into(), true), ("cover.png".into(), false), ("empty".into(), true)]
    );
    assert_eq!(listed(&zip, "", true).unwrap().len(), 4, "hidden entries on request");
    let inner = listed(&zip, "2025", false).unwrap();
    assert_eq!(names(&inner), [("a.png".into(), false), ("trip".into(), true)]);
    assert_eq!(inner.iter().find(|entry| entry.name == "a.png").unwrap().size, Some(4));
    assert_eq!(listed(&zip, "empty", false).unwrap(), []);
    assert_eq!(listed(&zip, "missing", false).unwrap_err().kind(), io::ErrorKind::NotFound);
}

#[test]
fn members_that_reach_outside_are_left_out() {
    let dir = TempDir::new("archive-escape");
    let tar = dir.join("evil.tar");
    // tar's builder refuses "..", so the names are written raw.
    let mut out = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut out);
        for name in ["../escaped", "/etc/escaped", "kept.txt"] {
            let mut header = tar::Header::new_gnu();
            header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name.as_bytes());
            header.set_size(2);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append(&header, &b"hi"[..]).unwrap();
        }
        builder.finish().unwrap();
    }
    fs::write(&tar, out).unwrap();
    assert_eq!(names(&listed(&tar, "", false).unwrap()), [("kept.txt".into(), false)]);
    unpacked(&tar, &dir.join("out"));
    assert_eq!(fs::read_dir(dir.join("out")).unwrap().count(), 1);
    assert!(!dir.join("escaped").exists());
}

#[test]
fn a_link_cant_carry_a_later_member_outside() {
    let dir = TempDir::new("archive-link");
    let outside = dir.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let zip = dir.join("links.zip");
    let target = format!("-> {}", outside.display());
    write_zip(&zip, &[("way", &target), ("way/planted", "x"), ("docs/readme", "hello"), ("docs/latest", "-> readme")]);
    unpacked(&zip, &dir.join("out"));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0, "nothing written through the link");
    assert_eq!(fs::read_to_string(dir.join("out/way/planted")).unwrap(), "x", "it's a folder of its own");
    assert_eq!(fs::read_link(dir.join("out/docs/latest")).unwrap(), Path::new("readme"));
    let mode = fs::metadata(dir.join("out/docs/readme")).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode, 0o755, "no setuid from the archive");
}

#[test]
fn every_tar_compression_extracts() {
    let dir = TempDir::new("archive-tars");
    let members = [("folder/", ""), ("folder/a.txt", "alpha"), ("folder/link", "-> a.txt")];
    let plain = write_tar(Vec::new(), &members);
    fs::write(dir.join("plain.tar"), &plain).unwrap();
    let gz = write_tar(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()), &members);
    fs::write(dir.join("gz.tar.gz"), gz.finish().unwrap()).unwrap();
    let mut zst = Vec::new();
    ruzstd::encoding::compress(&plain[..], &mut zst, ruzstd::encoding::CompressionLevel::Fastest);
    fs::write(dir.join("zst.tar.zst"), zst).unwrap();
    for name in ["plain.tar", "gz.tar.gz", "zst.tar.zst"] {
        let into = dir.join(format!("{name}-out"));
        unpacked(&dir.join(name), &into);
        assert_eq!(fs::read_to_string(into.join("folder/a.txt")).unwrap(), "alpha", "{name}");
        assert_eq!(fs::read_link(into.join("folder/link")).unwrap(), Path::new("a.txt"), "{name}");
        let modified = fs::metadata(into.join("folder/a.txt")).unwrap().modified().unwrap();
        assert_eq!(modified, UNIX_EPOCH + Duration::from_secs(1_700_000_000), "{name}");
        assert_eq!(
            names(&listed(&dir.join(name), "folder", false).unwrap()),
            [("a.txt".into(), false), ("link".into(), false)]
        );
    }
}

#[test]
fn a_packed_zip_extracts_to_what_was_packed() {
    let dir = TempDir::new("archive-pack");
    fs::create_dir_all(dir.join("from/Photos/2025")).unwrap();
    fs::write(dir.join("from/Photos/2025/a.png"), "png").unwrap();
    fs::write(dir.join("from/notes.txt"), "hi").unwrap();
    std::os::unix::fs::symlink("2025/a.png", dir.join("from/Photos/latest")).unwrap();
    let zip = dir.join("packed.zip");
    let mut packed_bytes = 0;
    let sources = [dir.join("from/Photos"), dir.join("from/notes.txt")];
    let sources: Vec<_> = sources.iter().map(PathBuf::as_path).collect();
    assert!(
        pack(&sources, fs::File::create(&zip).unwrap(), &mut |bytes| {
            packed_bytes += bytes;
            true
        })
        .unwrap()
    );
    assert_eq!(packed_bytes, 5);
    unpacked(&zip, &dir.join("out"));
    assert_eq!(fs::read_to_string(dir.join("out/Photos/2025/a.png")).unwrap(), "png");
    assert_eq!(fs::read_to_string(dir.join("out/notes.txt")).unwrap(), "hi");
    assert_eq!(fs::read_link(dir.join("out/Photos/latest")).unwrap(), Path::new("2025/a.png"));
}

#[test]
fn a_file_opened_from_a_zip_is_a_read_only_copy() {
    let dir = TempDir::new("archive-open");
    write_zip(&dir.join("docs.zip"), &[("2025/report.txt", "report")]);
    let inside = Inside::of(&dir.join("docs.zip/2025/report.txt")).unwrap();
    let first = open_copy(&inside, &dir.join("opened")).unwrap();
    let second = open_copy(&inside, &dir.join("opened")).unwrap();
    assert_ne!(first, second, "each copy in its own folder");
    assert_eq!(fs::read_to_string(&first).unwrap(), "report");
    assert_eq!(fs::metadata(&first).unwrap().permissions().mode() & 0o777, 0o444);
    let missing = Inside::of(&dir.join("docs.zip/2025/gone.txt")).unwrap();
    assert!(open_copy(&missing, &dir.join("opened")).is_err());
    assert_eq!(fs::read_dir(dir.join("opened")).unwrap().count(), 2, "a failed copy leaves no folder");
}

#[test]
fn a_member_listed_twice_ends_as_the_later_one_even_read_only() {
    let dir = TempDir::new("archive-twice");
    let mut tar = tar::Builder::new(Vec::new());
    for contents in ["old", "new"] {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o444);
        header.set_size(3);
        tar.append_data(&mut header, "notes.txt", contents.as_bytes()).unwrap();
    }
    fs::write(dir.join("twice.tar"), tar.into_inner().unwrap()).unwrap();
    unpacked(&dir.join("twice.tar"), &dir.join("out"));
    assert_eq!(fs::read_to_string(dir.join("out/notes.txt")).unwrap(), "new");
    let inside = Inside::of(&dir.join("twice.tar/notes.txt")).unwrap();
    assert_eq!(fs::read_to_string(open_copy(&inside, &dir.join("opened")).unwrap()).unwrap(), "new");
    assert_eq!(folder_name(OsStr::new("..zip")), "..zip", "never a folder named '.'");
}
