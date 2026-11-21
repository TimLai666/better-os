//! The accounts the login screen offers (design/login-screen-spec.md sections
//! 3.1, 3.3, 3.5 and 5.8).
//!
//! Accounts are what AccountsService lists (`ListCachedUsers`, which leaves
//! out system and hidden accounts); the greeter never reads `/etc/passwd`.
//! The last login comes first, then the rest by name. A picture is the file
//! AccountsService's `IconFile` names under `/var/lib/AccountsService/icons/`,
//! decoded under the spec's limits: at most 1 MiB and 1024 × 1024. Another
//! local account controls that file, so it is untrusted input.

use std::cmp::Ordering;
use std::io::Read;
use std::path::{Component, Path};

/// Where AccountsService keeps pictures; the only directory a picture is read
/// from.
pub const AVATAR_DIRECTORY: &str = "/var/lib/AccountsService/icons";
/// The largest picture file read.
pub const AVATAR_MAX_BYTES: u64 = 1024 * 1024;
/// The largest picture decoded, in pixels each way.
pub const AVATAR_MAX_SIDE: u32 = 1024;
/// How many fills an account without a picture can get its initial on.
pub const AVATAR_TINTS: u8 = 8;

/// One account, from AccountsService's D-Bus properties.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Account {
    /// `UserName`.
    pub user_name: String,
    /// `RealName`; may be empty.
    pub real_name: String,
    /// `LoginTime`, seconds since the epoch; 0 for never.
    pub login_time: i64,
    /// `IconFile`; may be empty or name a file that does not exist.
    pub icon_file: String,
    /// `Session`: the session last used, a `.desktop` file's id.
    pub session: String,
    /// `Language`.
    pub language: String,
}

impl Account {
    /// The name the card shows: the real name, or the account name when
    /// there is none.
    pub fn display_name(&self) -> &str {
        let real = self.real_name.trim();
        if real.is_empty() {
            &self.user_name
        } else {
            real
        }
    }

    /// The letter drawn when there is no picture. A Chinese, Japanese or
    /// Korean name shows its last character, the given name's, as the
    /// mockups draw 林小明 as 「明」; any other name its first letter in
    /// capitals.
    pub fn initial(&self) -> String {
        let name = self.display_name();
        let all_cjk = name
            .chars()
            .filter(|character| !character.is_whitespace())
            .all(is_cjk);
        let chosen = if all_cjk {
            name.chars()
                .rev()
                .find(|character| !character.is_whitespace())
        } else {
            name.chars().find(|character| character.is_alphanumeric())
        };
        chosen
            .map(|character| character.to_uppercase().collect())
            .unwrap_or_default()
    }

    /// Which of [`AVATAR_TINTS`] fills the initial sits on, derived from the
    /// account name so it is the same at every login.
    pub fn tint(&self) -> u8 {
        // FNV-1a: stable across builds and platforms, unlike `DefaultHasher`.
        let mut hash: u32 = 0x811c_9dc5;
        for byte in self.user_name.as_bytes() {
            hash ^= u32::from(*byte);
            hash = hash.wrapping_mul(0x0100_0193);
        }
        (hash % u32::from(AVATAR_TINTS)) as u8
    }
}

fn is_cjk(character: char) -> bool {
    matches!(character as u32,
        0x3040..=0x30ff      // Hiragana, Katakana
        | 0x3400..=0x4dbf    // CJK Extension A
        | 0x4e00..=0x9fff    // CJK Unified Ideographs
        | 0xac00..=0xd7af    // Hangul syllables
        | 0xf900..=0xfaff    // CJK Compatibility Ideographs
        | 0x20000..=0x2fa1f) // Extensions B to F, compatibility supplement
}

fn by_name(left: &Account, right: &Account) -> Ordering {
    let left_name = left.display_name().to_lowercase();
    let right_name = right.display_name().to_lowercase();
    left_name
        .cmp(&right_name)
        .then_with(|| left.user_name.cmp(&right.user_name))
}

/// Sorts accounts in the chooser's order: the most recent login first, then
/// by name. Accounts that never logged in follow every one that did.
pub fn order_accounts(accounts: &mut [Account]) {
    accounts.sort_by(|left, right| {
        right
            .login_time
            .max(0)
            .cmp(&left.login_time.max(0))
            .then_with(|| by_name(left, right))
    });
}

/// Whether a picture path is one the screen may read: absolute, with no `..`
/// or `.`, directly or below [`AVATAR_DIRECTORY`].
pub fn avatar_path_allowed(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
        && path.starts_with(AVATAR_DIRECTORY)
        && path != Path::new(AVATAR_DIRECTORY)
}

/// A decoded picture, eight-bit RGBA.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Avatar {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Why no picture is shown; the screen then draws the initial.
#[derive(Debug, thiserror::Error)]
pub enum AvatarError {
    #[error("the picture is outside {AVATAR_DIRECTORY}")]
    OutsideDirectory,
    #[error("the picture is larger than {AVATAR_MAX_BYTES} bytes")]
    TooLarge,
    #[error("the picture is larger than {AVATAR_MAX_SIDE} pixels each way")]
    TooManyPixels,
    #[error("the picture could not be read: {0}")]
    Read(#[from] std::io::Error),
    #[error("the picture could not be decoded: {0}")]
    Decode(#[from] image::ImageError),
}

/// Decodes a picture's bytes under the limits. Only PNG and JPEG are
/// understood.
pub fn decode_avatar(bytes: &[u8]) -> Result<Avatar, AvatarError> {
    if bytes.len() as u64 > AVATAR_MAX_BYTES {
        return Err(AvatarError::TooLarge);
    }
    // The header first, so a picture over the limit is refused before any
    // pixel is decoded.
    let mut header = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    header.limits(decode_limits());
    let (width, height) = header.into_dimensions().map_err(limit_error)?;
    if width > AVATAR_MAX_SIDE || height > AVATAR_MAX_SIDE {
        return Err(AvatarError::TooManyPixels);
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    reader.limits(decode_limits());
    let image = reader.decode().map_err(limit_error)?.into_rgba8();
    Ok(Avatar {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

fn decode_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(AVATAR_MAX_SIDE);
    limits.max_image_height = Some(AVATAR_MAX_SIDE);
    // Four bytes a pixel at the largest size, with as much again for the
    // decoder's own buffers.
    limits.max_alloc = Some(2 * 4 * u64::from(AVATAR_MAX_SIDE) * u64::from(AVATAR_MAX_SIDE));
    limits
}

fn limit_error(error: image::ImageError) -> AvatarError {
    match error {
        image::ImageError::Limits(_) => AvatarError::TooManyPixels,
        other => AvatarError::Decode(other),
    }
}

/// Reads and decodes the picture an account's `IconFile` names, reading at
/// most one byte past the limit so a huge file is refused without being read.
pub fn read_avatar(path: &Path) -> Result<Avatar, AvatarError> {
    if !avatar_path_allowed(path) {
        return Err(AvatarError::OutsideDirectory);
    }
    read_avatar_unchecked(path)
}

fn read_avatar_unchecked(path: &Path) -> Result<Avatar, AvatarError> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(AVATAR_MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > AVATAR_MAX_BYTES {
        return Err(AvatarError::TooLarge);
    }
    decode_avatar(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(user: &str, real: &str, login: i64) -> Account {
        Account {
            user_name: user.into(),
            real_name: real.into(),
            login_time: login,
            ..Account::default()
        }
    }

    #[test]
    fn last_login_first_then_by_name() {
        let mut accounts = vec![
            account("chen", "陳美玲", 0),
            account("wang", "王大同", 1_700_000_000),
            account("lin", "林小明", 1_800_000_000),
            account("bob", "", 0),
            account("alice", "Alice", 0),
            account("amy", "alice", 0),
        ];
        order_accounts(&mut accounts);
        let names: Vec<&str> = accounts.iter().map(|a| a.user_name.as_str()).collect();
        assert_eq!(names, ["lin", "wang", "alice", "amy", "bob", "chen"]);
    }

    #[test]
    fn the_initial_follows_the_mockups() {
        assert_eq!(account("lin", "林小明", 0).initial(), "明");
        assert_eq!(account("wang", "王 大同", 0).initial(), "同");
        assert_eq!(account("tim", "tim lai", 0).initial(), "T");
        assert_eq!(account("tim", "", 0).initial(), "T");
        assert_eq!(account("x", "  ", 0).display_name(), "x");
        assert_eq!(account("", "", 0).initial(), "");
        assert_eq!(account("élise", "", 0).initial(), "É");
    }

    #[test]
    fn the_tint_is_stable_and_in_range() {
        let tim = account("tim", "", 0);
        assert_eq!(tim.tint(), tim.tint());
        for name in ["a", "b", "lin", "wang", "chen", "root", ""] {
            assert!(account(name, "", 0).tint() < AVATAR_TINTS);
        }
        // FNV-1a of "a" is 0xe40c292c.
        assert_eq!(account("a", "", 0).tint(), (0xe40c_292c_u32 % 8) as u8);
    }

    #[test]
    fn pictures_are_read_only_from_accountsservice() {
        assert!(avatar_path_allowed(Path::new(
            "/var/lib/AccountsService/icons/tim"
        )));
        assert!(!avatar_path_allowed(Path::new(
            "/var/lib/AccountsService/icons"
        )));
        assert!(!avatar_path_allowed(Path::new(
            "/var/lib/AccountsService/icons/../users/tim"
        )));
        assert!(!avatar_path_allowed(Path::new("/home/tim/.face")));
        assert!(!avatar_path_allowed(Path::new(
            "var/lib/AccountsService/icons/tim"
        )));
        assert!(matches!(
            read_avatar(Path::new("/home/tim/.face")),
            Err(AvatarError::OutsideDirectory)
        ));
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn a_picture_within_the_limits_decodes() {
        let avatar = decode_avatar(&png(96, 64)).unwrap();
        assert_eq!((avatar.width, avatar.height), (96, 64));
        assert_eq!(avatar.rgba.len(), 96 * 64 * 4);
        assert_eq!(&avatar.rgba[..4], &[10, 20, 30, 255]);
        let largest = decode_avatar(&png(1024, 1024)).unwrap();
        assert_eq!(largest.width, 1024);
    }

    #[test]
    fn a_picture_over_the_pixel_limit_is_refused() {
        assert!(matches!(
            decode_avatar(&png(1025, 8)),
            Err(AvatarError::TooManyPixels)
        ));
        assert!(matches!(
            decode_avatar(&png(8, 1025)),
            Err(AvatarError::TooManyPixels)
        ));
    }

    #[test]
    fn a_picture_over_the_byte_limit_is_refused_unread() {
        let big = vec![0u8; AVATAR_MAX_BYTES as usize + 1];
        assert!(matches!(decode_avatar(&big), Err(AvatarError::TooLarge)));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge");
        std::fs::write(&path, &big).unwrap();
        assert!(matches!(
            read_avatar_unchecked(&path),
            Err(AvatarError::TooLarge)
        ));
        let small = dir.path().join("small");
        std::fs::write(&small, png(4, 4)).unwrap();
        assert_eq!(read_avatar_unchecked(&small).unwrap().width, 4);
    }

    #[test]
    fn something_that_is_not_a_picture_is_refused() {
        assert!(matches!(
            decode_avatar(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
            Err(AvatarError::Decode(_))
        ));
        assert!(decode_avatar(&[]).is_err());
    }
}
