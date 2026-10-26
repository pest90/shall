//! Creation of new items: generated IDs and item skeletons.
//!
//! An ID is `typ~base-random~1`, where the base is the slug of the title,
//! optionally behind a component. The base makes the ID readable, the
//! random part makes IDs from parallel branches practically collision-free.
//! This module is free of I/O: the caller supplies the random source and
//! decides which IDs are taken, e.g. by looking at the files of the
//! project and at the Git history.

use thiserror::Error;

use crate::id::{ItemId, ItemType, ParseIdError};

/// Alphabet of the random part: Crockford's Base32 in lower case, without
/// `i`, `l`, `o` and `u`, which are easily confused or misread.
pub const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Maximum length of the slug taken from the title.
pub const MAX_SLUG_LEN: usize = 32;

/// Prefix of slugs that would not start with a letter.
const SLUG_PREFIX: &str = "item";

/// Attempts per length of the random part before it grows by one.
const ATTEMPTS_PER_LEN: usize = 20;

/// Lengths of the random part that are tried in turn.
const RANDOM_LENS: [usize; 2] = [4, 5];

/// Turns a title into a slug: transliterated to ASCII, lower case, words
/// joined by `-`, at most [`MAX_SLUG_LEN`] characters and cut at a word
/// boundary. A slug that would not start with a letter gets the prefix
/// `item-`, because names must start with a letter.
pub fn slug(title: &str) -> String {
    let words = words(title);
    let starts_with_letter = words
        .first()
        .is_some_and(|word| word.starts_with(|c: char| c.is_ascii_alphabetic()));
    let mut slug = String::new();
    if !starts_with_letter {
        slug.push_str(SLUG_PREFIX);
    }
    for word in words {
        let separator = usize::from(!slug.is_empty());
        if slug.len() + separator + word.len() > MAX_SLUG_LEN {
            if slug.is_empty() {
                // A single overlong word is cut instead of dropped.
                slug.push_str(&word[..MAX_SLUG_LEN]);
            }
            break;
        }
        if separator == 1 {
            slug.push('-');
        }
        slug.push_str(&word);
    }
    slug
}

/// Splits a title into lower-case ASCII words. Apostrophes join, every
/// other character that is no letter or digit separates.
fn words(title: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in title.chars() {
        if let Some(ascii) = transliterate(c) {
            word.push_str(ascii);
        } else if c.is_ascii_alphanumeric() {
            word.push(c.to_ascii_lowercase());
        } else if c == '\'' || c == '\u{2019}' {
            // "don't" becomes "dont", not "don-t".
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Lower-case ASCII spelling of common Latin letters with diacritics.
fn transliterate(c: char) -> Option<&'static str> {
    Some(match c.to_lowercase().next().unwrap_or(c) {
        'ä' | 'æ' => "ae",
        'ö' | 'ø' | 'œ' => "oe",
        'ü' => "ue",
        'ß' => "ss",
        'à' | 'á' | 'â' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'ç' | 'ć' | 'č' => "c",
        'ď' | 'đ' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
        'ğ' => "g",
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
        'ł' | 'ľ' | 'ĺ' => "l",
        'ñ' | 'ń' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ō' | 'ő' => "o",
        'ŕ' | 'ř' => "r",
        'ś' | 'š' | 'ş' | 'ș' => "s",
        'ť' | 'ţ' | 'ț' => "t",
        'ù' | 'ú' | 'û' | 'ū' | 'ů' | 'ű' | 'ų' => "u",
        'ý' | 'ÿ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

/// Returns the readable part of a new ID: the slug of the title, behind
/// the component and a `.` if one is given.
///
/// The title must not be empty and must not contain line breaks or other
/// control characters, since it becomes a heading. The component is
/// lower-cased and consists of `.`-separated segments of ASCII letters,
/// digits, `_` and `-`, starting with a letter, e.g. `motor` or `can.rx`.
pub fn base_name(title: &str, component: Option<&str>) -> Result<String, CreateError> {
    if title.trim().is_empty() || title.chars().any(char::is_control) {
        return Err(CreateError::InvalidTitle(title.to_owned()));
    }
    let slug = slug(title);
    let Some(component) = component else {
        return Ok(slug);
    };
    let lower = component.to_ascii_lowercase();
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    if !lower.starts_with(|c: char| c.is_ascii_alphabetic()) || !lower.split('.').all(valid_segment)
    {
        return Err(CreateError::InvalidComponent(component.to_owned()));
    }
    Ok(format!("{lower}.{slug}"))
}

/// Generates a new ID of revision 1 from a base made by [`base_name`].
///
/// `random_index` must return a uniformly distributed index into
/// [`ALPHABET`]. `is_taken` decides whether an ID is already in use, with
/// any revision; taken IDs are never returned.
pub fn generate_id(
    typ: &ItemType,
    base: &str,
    mut random_index: impl FnMut() -> usize,
    mut is_taken: impl FnMut(&ItemId) -> bool,
) -> Result<ItemId, CreateError> {
    for len in RANDOM_LENS {
        for _ in 0..ATTEMPTS_PER_LEN {
            let random: String = (0..len)
                .map(|_| char::from(ALPHABET[random_index() % ALPHABET.len()]))
                .collect();
            let id = ItemId::new(typ.clone(), &format!("{base}-{random}"), 1)?;
            if !is_taken(&id) {
                return Ok(id);
            }
        }
    }
    Err(CreateError::Exhausted {
        base: base.to_owned(),
        attempts: ATTEMPTS_PER_LEN * RANDOM_LENS.len(),
    })
}

/// Returns the Markdown skeleton of a new item, starting with a blank line,
/// for appending to a requirements file.
///
/// The `Covers:` entry and the description are placeholders that the
/// author replaces; the placeholder entry is reported as an invalid ID
/// until then. `Covers:` is left out for top-level types.
pub fn skeleton(id: &ItemId, title: &str, needs: &[ItemType], has_parents: bool) -> String {
    let title = title.trim();
    let mut text = format!("\n## {title}\n`{id}`\n\nTODO: describe the requirement.\n");
    if has_parents {
        text.push_str("\nCovers:\n- TODO-parent-id\n");
    }
    if !needs.is_empty() {
        let needs: Vec<&str> = needs.iter().map(ItemType::as_str).collect();
        text.push_str(&format!("\nNeeds: {}\n", needs.join(", ")));
    }
    text
}

/// Error when creating an item.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CreateError {
    /// The title is empty or contains control characters.
    #[error("invalid title {0:?}: it must not be empty or contain line breaks")]
    InvalidTitle(String),
    /// The component is not a valid start of a name.
    #[error(
        "invalid component '{0}': expected segments of letters, digits, '_' and '-', \
         separated by '.' and starting with a letter"
    )]
    InvalidComponent(String),
    /// The generated name is not a valid name.
    #[error(transparent)]
    InvalidId(#[from] ParseIdError),
    /// Every generated ID was taken.
    #[error("could not find a free ID for '{base}' in {attempts} attempts")]
    Exhausted {
        /// The base of the IDs that were tried.
        base: String,
        /// Number of IDs tried.
        attempts: usize,
    },
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::markdown;

    fn swr() -> ItemType {
        "swr".parse().unwrap()
    }

    /// Random source that yields the indices of the given characters.
    fn sequence(chars: &str) -> impl FnMut() -> usize {
        let indices: Vec<usize> = chars
            .bytes()
            .map(|b| ALPHABET.iter().position(|&a| a == b).unwrap())
            .collect();
        let mut next = 0;
        move || {
            let index = indices[next % indices.len()];
            next += 1;
            index
        }
    }

    #[test]
    fn slugs_titles() {
        assert_eq!(
            slug("Motor bei Übertemperatur abschalten"),
            "motor-bei-uebertemperatur"
        );
        assert_eq!(
            slug("  Shut down -- on   overtemp!  "),
            "shut-down-on-overtemp"
        );
        assert_eq!(slug("Größe für Café"), "groesse-fuer-cafe");
        assert_eq!(slug("Łódź Straße, Čech"), "lodz-strasse-cech");
        assert_eq!(slug("Don't stop"), "dont-stop");
        assert_eq!(slug("10 ms task"), "item-10-ms-task");
        assert_eq!(slug("€€€"), "item");
        assert_eq!(slug("Привет"), "item");
        let long = slug("Übertemperaturabschaltungsüberwachungsfunktion");
        assert_eq!(long, "uebertemperaturabschaltungsueber");
        assert_eq!(long.len(), MAX_SLUG_LEN);
        let prefixed = slug("1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16");
        assert!(prefixed.len() <= MAX_SLUG_LEN, "{prefixed}");
        assert!(prefixed.starts_with("item-1-2-3"), "{prefixed}");
    }

    #[test]
    fn every_title_gives_a_valid_id() {
        let titles = [
            "x",
            "X",
            "1",
            "-",
            "💥 boom",
            "a\u{301}b",
            "日本語",
            "C# ## x",
            "İstanbul",
            "a'b",
            "Übertemperaturabschaltungsüberwachungsfunktion und mehr",
        ];
        for title in titles {
            let base = base_name(title, Some("Motor.Rx_1")).unwrap();
            let id = generate_id(&swr(), &base, sequence("k4f2"), |_| false).unwrap();
            let reparsed: ItemId = id.to_string().parse().unwrap();
            assert_eq!(reparsed, id, "{title:?}");
            assert!(id.name().starts_with("motor.rx_1."), "{id}");
        }
    }

    #[test]
    fn builds_base_names() {
        assert_eq!(
            base_name("Overtemp shutdown", Some("motor")).unwrap(),
            "motor.overtemp-shutdown"
        );
        assert_eq!(
            base_name("Overtemp", Some("CAN.rx")).unwrap(),
            "can.rx.overtemp"
        );
        assert_eq!(base_name("Overtemp", None).unwrap(), "overtemp");
        for title in ["", "   ", "x\n`swr~evil~1`", "tab\there"] {
            assert_eq!(
                base_name(title, None),
                Err(CreateError::InvalidTitle(title.to_owned())),
            );
        }
        for component in ["", "1motor", "motor..x", ".motor", "motor/1", "mötor"] {
            assert_eq!(
                base_name("x", Some(component)),
                Err(CreateError::InvalidComponent(component.to_owned())),
                "{component}",
            );
        }
    }

    #[test]
    fn generates_ids() {
        let id = generate_id(&swr(), "motor.overtemp-shutdown", sequence("k4f2"), |_| {
            false
        })
        .unwrap();
        assert_eq!(id.to_string(), "swr~motor.overtemp-shutdown-k4f2~1");
    }

    #[test]
    fn never_returns_taken_ids() {
        // `aaaa` was deleted but is still in the history, so it is taken.
        let deleted = "swr~x-aaaa~3".parse::<ItemId>().unwrap();
        let id = generate_id(&swr(), "x", sequence("aaaabbbb"), |id| {
            id.same_item(&deleted)
        })
        .unwrap();
        assert_eq!(id.to_string(), "swr~x-bbbb~1");
    }

    #[test]
    fn grows_random_part_when_short_ids_are_taken() {
        let id = generate_id(&swr(), "x", sequence("a"), |id| {
            id.name().len() < "x-aaaaa".len()
        })
        .unwrap();
        assert_eq!(id.to_string(), "swr~x-aaaaa~1");
        let error = generate_id(&swr(), "x", sequence("a"), |_| true).unwrap_err();
        assert_eq!(
            error,
            CreateError::Exhausted {
                base: "x".to_owned(),
                attempts: 40
            }
        );
    }

    #[test]
    fn generates_ten_thousand_unique_ids() {
        // With 32^4 random parts, about 50 of 10 000 draws collide with an
        // earlier one; each collision must be re-rolled, never returned.
        let mut taken = HashSet::new();
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut random = move || {
            // xorshift64: deterministic and uniform enough for this test.
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 32) as usize
        };
        let mut collisions = 0;
        for _ in 0..10_000 {
            let id = generate_id(&swr(), "same-title", &mut random, |id| {
                let taken = taken.contains(id);
                collisions += usize::from(taken);
                taken
            })
            .unwrap();
            assert!(taken.insert(id));
        }
        assert_eq!(taken.len(), 10_000);
        assert!(collisions > 0, "the test must exercise re-rolls");
    }

    #[test]
    fn skeleton_parses_back() {
        let id = "dsn~motor.x-k4f2~1".parse().unwrap();
        let needs = ["impl".parse().unwrap(), "utest".parse().unwrap()];
        let text = skeleton(&id, "Temperature monitor", &needs, true);
        assert_eq!(
            text,
            "\n## Temperature monitor\n`dsn~motor.x-k4f2~1`\n\nTODO: describe the requirement.\n\
             \nCovers:\n- TODO-parent-id\n\nNeeds: impl, utest\n"
        );
        let parsed = markdown::parse(std::path::Path::new("x.md"), &text).unwrap();
        assert_eq!(parsed.items[0].id, id);
        assert_eq!(parsed.items[0].title, "Temperature monitor");
        assert_eq!(parsed.items[0].needs, needs);
        let messages: Vec<_> = parsed.diagnostics.iter().map(ToString::to_string).collect();
        assert_eq!(
            messages,
            ["x.md:8: invalid ID 'TODO-parent-id': expected 'typ~name~revision'"]
        );

        let top = skeleton(&id, "Top", &[], false);
        assert_eq!(
            top,
            "\n## Top\n`dsn~motor.x-k4f2~1`\n\nTODO: describe the requirement.\n"
        );
    }
}
