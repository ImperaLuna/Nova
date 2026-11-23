use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const MOOV_MAX: u64 = 64 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Contents {
    Recorded,
    NoFrames,
    Unknown,
}

pub(crate) fn contents(path: &Path) -> Contents {
    File::open(path).ok().map_or(Contents::Unknown, |mut file| read(&mut file).unwrap_or(Contents::Unknown))
}

struct Box {
    kind: [u8; 4],
    start: u64,
    len: u64,
}

fn boxes_in(bytes: &[u8]) -> Vec<Box> {
    let mut found = Vec::new();
    let mut at = 0usize;
    while let Some(header) = at.checked_add(8).and_then(|end| bytes.get(at..end)) {
        let size = u64::from(u32::from_be_bytes([header[0], header[1], header[2], header[3]]));
        let kind = [header[4], header[5], header[6], header[7]];
        let (head, size) = match size {
            0 => (8, (bytes.len() - at) as u64),
            1 => match bytes.get(at + 8..at.saturating_add(16)) {
                Some(large) => (16, u64::from_be_bytes(large.try_into().unwrap_or_default())),
                None => break,
            },
            size => (8, size),
        };
        if size < head {
            break;
        }
        let Ok(end) = usize::try_from((at as u64).saturating_add(size)) else { break };
        if end > bytes.len() {
            break;
        }
        found.push(Box { kind, start: (at as u64) + head, len: size - head });
        at = end;
    }
    found
}

fn read(file: &mut File) -> std::io::Result<Contents> {
    let file_len = file.metadata()?.len();
    let mut at = 0u64;
    let mut first = true;
    let mut media = 0u64;
    let mut moov = None;
    while at + 8 <= file_len {
        let mut header = [0u8; 16];
        file.seek(SeekFrom::Start(at))?;
        let got = file.read(&mut header)?;
        let mut size = u64::from(u32::from_be_bytes([header[0], header[1], header[2], header[3]]));
        let kind = [header[4], header[5], header[6], header[7]];
        let mut head = 8;
        if size == 1 && got >= 16 {
            size = u64::from_be_bytes(header[8..16].try_into().unwrap_or_default());
            head = 16;
        }
        let rest = file_len - at;
        if size == 0 || size > rest || size < head {
            size = rest;
        }
        if first && &kind != b"ftyp" {
            return Ok(Contents::Unknown);
        }
        first = false;
        match &kind {
            b"mdat" => media += size.saturating_sub(head),
            b"moof" => return Ok(Contents::Recorded),
            b"moov" => moov = Some((at + head, size.saturating_sub(head))),
            _ => {}
        }
        at += size;
    }
    if first {
        return Ok(Contents::Unknown);
    }
    let Some((start, len)) = moov else {
        return Ok(if media > 0 { Contents::Recorded } else { Contents::NoFrames });
    };
    if len > MOOV_MAX {
        return Ok(Contents::Recorded);
    }
    let mut index = vec![0u8; usize::try_from(len).unwrap_or(0)];
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut index)?;
    Ok(index_contents(&index))
}

fn index_contents(moov: &[u8]) -> Contents {
    let mut unreadable = false;
    for track in boxes_in(moov).iter().filter(|b| &b.kind == b"trak") {
        let Some(mdia) = child(moov, track, b"mdia") else {
            unreadable = true;
            continue;
        };
        let picture = child(moov, &mdia, b"hdlr").and_then(|hdlr| payload(moov, &hdlr).get(8..12)) == Some(b"vide");
        if !picture {
            continue;
        }
        let stsz = child(moov, &mdia, b"minf")
            .and_then(|minf| child(moov, &minf, b"stbl"))
            .and_then(|stbl| child(moov, &stbl, b"stsz"));
        match stsz.and_then(|stsz| payload(moov, &stsz).get(8..12).map(<[u8]>::to_vec)) {
            Some(count) if u32::from_be_bytes([count[0], count[1], count[2], count[3]]) > 0 => {
                return Contents::Recorded;
            }
            Some(_) => {}
            None => unreadable = true,
        }
    }
    if unreadable { Contents::Unknown } else { Contents::NoFrames }
}

fn payload<'a>(bytes: &'a [u8], of: &Box) -> &'a [u8] {
    let start = usize::try_from(of.start).unwrap_or(usize::MAX);
    let end = start.saturating_add(usize::try_from(of.len).unwrap_or(usize::MAX));
    bytes.get(start..end.min(bytes.len())).unwrap_or_default()
}

fn child(bytes: &[u8], parent: &Box, kind: &[u8; 4]) -> Option<Box> {
    boxes_in(payload(bytes, parent)).into_iter().find(|b| &b.kind == kind).map(|b| Box {
        kind: b.kind,
        start: parent.start + b.start,
        len: b.len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorded(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/recorded").join(name)
    }

    #[test]
    fn a_recording_with_frames_is_one() {
        assert_eq!(contents(&recorded("wf-recorder-saved.mp4")), Contents::Recorded);
    }

    #[test]
    fn a_header_left_by_a_recorder_stalled_on_sound_has_no_frames() {
        let path = recorded("wf-recorder-stalled-on-sound.mp4");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 48);
        assert_eq!(contents(&path), Contents::NoFrames);
    }

    #[test]
    fn a_finished_file_with_sound_and_no_picture_has_no_frames() {
        assert_eq!(contents(&recorded("wf-recorder-sound-only.mp4")), Contents::NoFrames);
    }

    #[test]
    fn a_file_cut_off_after_some_frames_is_kept() {
        let dir = nova_test_support::TempDir::new("mp4-cut");
        let whole = std::fs::read(recorded("wf-recorder-saved.mp4")).unwrap();
        let moov = whole.windows(4).position(|w| w == b"moov").unwrap() - 4;
        let cut = dir.join("cut.mp4");
        std::fs::write(&cut, &whole[..moov]).unwrap();
        assert_eq!(contents(&cut), Contents::Recorded);
    }

    #[test]
    fn frames_after_a_media_box_left_open_are_kept() {
        let dir = nova_test_support::TempDir::new("mp4-open");
        let mut file = std::fs::read(recorded("wf-recorder-stalled-on-sound.mp4")).unwrap();
        let whole = std::fs::read(recorded("wf-recorder-saved.mp4")).unwrap();
        let frames = whole.windows(4).position(|w| w == b"mdat").unwrap() + 4;
        file.extend_from_slice(&whole[frames..frames + 4096]);
        let open = dir.join("open.mp4");
        std::fs::write(&open, &file).unwrap();
        assert_eq!(contents(&open), Contents::Recorded);
    }

    #[test]
    fn a_box_claiming_more_than_there_is_ends_the_walk() {
        let mut index = vec![0, 0, 0, 1];
        index.extend_from_slice(b"trak");
        index.extend_from_slice(&u64::MAX.to_be_bytes());
        assert!(boxes_in(&index).is_empty());
        assert_eq!(index_contents(&index), Contents::NoFrames);
    }

    #[test]
    fn what_isnt_an_mp4_is_unknown() {
        let dir = nova_test_support::TempDir::new("mp4-other");
        let other = dir.join("other.mp4");
        std::fs::write(&other, "frames\nfinished\n").unwrap();
        assert_eq!(contents(&other), Contents::Unknown);
        assert_eq!(contents(&dir.join("missing.mp4")), Contents::Unknown);
    }
}
