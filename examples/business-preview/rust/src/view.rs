//! What the preview shows of a profile. Every rule about showing is here, so
//! SwiftUI only lays it out:
//!
//! - A price is shown only once a person has verified it; the others are
//!   counted, so the screen can say some are left out.
//! - While the business hasn't agreed (`preview.status` is `concept`), every
//!   screen carries the concept label.
//! - The words (Lessons, Book a lesson) come from the business's category.
//! - Calls, emails, maps and the book button are URLs made here; the book
//!   button only ever opens the business's own page or contacts it.

use std::path::Path;

use business_profile::{ActionKind, Category, Field, PreviewStatus, Profile, Weekday};

/// The label every screen of a concept preview carries, as the spec words it.
pub const CONCEPT_LABEL: &str = "Concept preview, not an official app";

/// One business, ready to show.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct BusinessView {
    pub id: String,
    pub name: String,
    pub tagline: Option<String>,
    pub description: Option<String>,
    /// The kind of business, in words ("Riding school").
    pub category: String,
    /// `#RRGGBB`, the business's own colours, as the site has them.
    pub primary_color: Option<String>,
    pub accent_color: Option<String>,
    /// The brand colour made readable, for light and dark mode. None when
    /// the profile has no colour: the preview keeps ForgeUI's own.
    pub brand: Option<BrandColors>,
    /// Where the logo and hero image are on the phone.
    pub logo_path: Option<String>,
    pub hero_path: Option<String>,
    pub words: Words,
    pub services: Vec<ServiceView>,
    /// Prices on the site that no person has checked yet, so not shown.
    pub hidden_prices: u32,
    pub hours: Vec<DayHours>,
    pub contact: Vec<ContactView>,
    pub book: Option<BookView>,
    /// `Some(CONCEPT_LABEL)` on every screen while the business hasn't agreed.
    pub concept_label: Option<String>,
}

/// One brand colour as the screens use it, in light and dark mode, each as
/// `0xRRGGBB`: `solid` fills (the header, the book button), `on_solid` is text
/// on it, `text` is the colour as text on the screen's background. Each pair
/// passes WCAG AA (4.5:1): the site's colour is darkened (light mode) or
/// lightened (dark mode) only as far as that takes.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct BrandColors {
    pub light_solid: u32,
    pub light_on_solid: u32,
    pub light_text: u32,
    pub dark_solid: u32,
    pub dark_on_solid: u32,
    pub dark_text: u32,
}

/// The backgrounds text sits on: iOS's grouped background in light mode,
/// and its raised card colour in dark mode (the hardest of each).
const LIGHT_BACKGROUND: u32 = 0xF2F2F7;
const DARK_BACKGROUND: u32 = 0x2C2C2E;
const AA: f64 = 4.5;

fn channels(rgb: u32) -> [f64; 3] {
    [(rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF].map(|c| c as f64 / 255.0)
}

fn luminance(rgb: u32) -> f64 {
    let [r, g, b] = channels(rgb).map(|c| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG contrast ratio of two colours.
pub fn contrast(a: u32, b: u32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `rgb` mixed `t` (0 to 1) of the way to `towards`.
fn mix(rgb: u32, towards: u32, t: f64) -> u32 {
    let (a, b) = (channels(rgb), channels(towards));
    let c: Vec<u32> = (0..3)
        .map(|i| ((a[i] + (b[i] - a[i]) * t) * 255.0).round() as u32)
        .collect();
    (c[0] << 16) | (c[1] << 8) | c[2]
}

/// The least mix of `rgb` towards `towards` that reads at AA on `background`.
fn readable(rgb: u32, towards: u32, background: u32) -> u32 {
    (0..=20)
        .map(|step| mix(rgb, towards, step as f64 / 20.0))
        .find(|&c| contrast(c, background) >= AA)
        .unwrap_or(towards)
}

/// White or black, whichever reads better on `solid`.
fn on(solid: u32) -> u32 {
    if contrast(solid, 0xFFFFFF) >= contrast(solid, 0x000000) {
        0xFFFFFF
    } else {
        0x000000
    }
}

/// `#RRGGBB` as a number.
pub fn parse_hex(colour: &str) -> Option<u32> {
    let hex = colour.strip_prefix('#')?;
    (hex.len() == 6).then(|| u32::from_str_radix(hex, 16).ok())?
}

pub fn brand_colors(colour: u32) -> BrandColors {
    let light_text = readable(colour, 0x000000, LIGHT_BACKGROUND);
    let dark_text = readable(colour, 0xFFFFFF, DARK_BACKGROUND);
    // A solid keeps the site's colour when white or black text reads on it.
    let solid = |fallback: u32| {
        if contrast(colour, on(colour)) >= AA {
            colour
        } else {
            fallback
        }
    };
    let (light_solid, dark_solid) = (solid(light_text), solid(dark_text));
    BrandColors {
        light_solid,
        light_on_solid: on(light_solid),
        light_text,
        dark_solid,
        dark_on_solid: on(dark_solid),
        dark_text,
    }
}

/// The category's vocabulary.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Words {
    /// The Services screen's title: Lessons, Treatments, Services.
    pub services_title: String,
    /// The primary action: Book a lesson, Ask for a quote.
    pub book_label: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ServiceView {
    pub name: String,
    /// "£45.00", only when a person has verified it.
    pub price: Option<String>,
    /// "30 min", "1 hr 30 min".
    pub duration: Option<String>,
    pub notes: Option<String>,
}

/// A day's opening times, in week order.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct DayHours {
    pub day: String,
    /// "09:00 to 17:00", several joined with commas.
    pub times: String,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactKind {
    Phone,
    Email,
    Address,
    Website,
}

/// A way to reach the business, with the URL that does it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ContactView {
    pub kind: ContactKind,
    /// What the row shows: the number, the address, the site's name.
    pub label: String,
    /// `tel:`, `mailto:`, an Apple Maps link or the https page.
    pub url: String,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookKind {
    /// Their own booking page, opened in an in-app browser.
    Page,
    Call,
    Email,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct BookView {
    pub label: String,
    pub url: String,
    pub kind: BookKind,
}

pub fn category_name(category: Category) -> &'static str {
    match category {
        Category::RidingSchool => "Riding school",
        Category::HairAndBeauty => "Hair and beauty",
        Category::Trades => "Trades",
        Category::ProfessionalServices => "Professional services",
        Category::Other => "Local business",
    }
}

pub fn words(category: Category) -> Words {
    let (services, book) = match category {
        Category::RidingSchool => ("Lessons", "Book a Lesson"),
        Category::HairAndBeauty => ("Treatments", "Book an Appointment"),
        Category::Trades => ("Services", "Ask for a Quote"),
        Category::ProfessionalServices => ("Services", "Book an Appointment"),
        Category::Other => ("Services", "Get in Touch"),
    };
    Words {
        services_title: services.into(),
        book_label: book.into(),
    }
}

pub fn concept_label(profile: &Profile) -> Option<String> {
    (profile.preview.status == PreviewStatus::Concept).then(|| CONCEPT_LABEL.to_string())
}

/// "£45.00", "€12", "30.00 CHF".
pub fn format_price(amount: &str, currency: &str) -> String {
    match currency {
        "GBP" => format!("£{amount}"),
        "EUR" => format!("€{amount}"),
        "USD" => format!("${amount}"),
        _ => format!("{amount} {currency}"),
    }
}

/// "45 min", "1 hr", "1 hr 30 min".
pub fn format_duration(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

fn day_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Monday => "Monday",
        Weekday::Tuesday => "Tuesday",
        Weekday::Wednesday => "Wednesday",
        Weekday::Thursday => "Thursday",
        Weekday::Friday => "Friday",
        Weekday::Saturday => "Saturday",
        Weekday::Sunday => "Sunday",
    }
}

const WEEK: [Weekday; 7] = [
    Weekday::Monday,
    Weekday::Tuesday,
    Weekday::Wednesday,
    Weekday::Thursday,
    Weekday::Friday,
    Weekday::Saturday,
    Weekday::Sunday,
];

pub fn hours(periods: &[Field<business_profile::OpeningPeriod>]) -> Vec<DayHours> {
    WEEK.iter()
        .filter_map(|&day| {
            let mut times: Vec<(&str, &str)> = periods
                .iter()
                .filter(|p| p.value.day == day)
                .map(|p| (p.value.opens.as_str(), p.value.closes.as_str()))
                .collect();
            if times.is_empty() {
                return None;
            }
            times.sort();
            times.dedup();
            let times = times
                .iter()
                .map(|(o, c)| format!("{o} to {c}"))
                .collect::<Vec<_>>()
                .join(", ");
            Some(DayHours {
                day: day_name(day).into(),
                times,
            })
        })
        .collect()
}

/// A `tel:` URL from a number as the site writes it, or None if it has no digits.
pub fn tel_url(number: &str) -> Option<String> {
    let mut out = String::new();
    for c in number.trim().chars() {
        if c.is_ascii_digit() || (c == '+' && out.is_empty()) {
            out.push(c);
        }
    }
    out.chars()
        .any(|c| c.is_ascii_digit())
        .then(|| format!("tel:{out}"))
}

/// An Apple Maps search for the address.
pub fn maps_url(address: &str) -> String {
    let query: String = url::form_urlencoded::byte_serialize(address.as_bytes()).collect();
    format!("https://maps.apple.com/?q={query}")
}

/// What a link row shows: the host without `www.`.
fn site_name(link: &str) -> String {
    url::Url::parse(link)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_string())
        })
        .unwrap_or_else(|| link.to_string())
}

pub fn contact(profile: &Profile) -> Vec<ContactView> {
    let c = &profile.contact;
    let mut out = Vec::new();
    if let Some(phone) = &c.phone {
        if let Some(url) = tel_url(&phone.value) {
            out.push(ContactView {
                kind: ContactKind::Phone,
                label: phone.value.trim().to_string(),
                url,
            });
        }
    }
    if let Some(email) = &c.email {
        out.push(ContactView {
            kind: ContactKind::Email,
            label: email.value.clone(),
            url: format!("mailto:{}", email.value),
        });
    }
    if let Some(address) = &c.address {
        out.push(ContactView {
            kind: ContactKind::Address,
            label: address.value.clone(),
            url: maps_url(&address.value),
        });
    }
    for link in &c.social {
        out.push(ContactView {
            kind: ContactKind::Website,
            label: site_name(&link.value),
            url: link.value.clone(),
        });
    }
    out
}

pub fn book(profile: &Profile) -> Option<BookView> {
    let action = profile.actions.book.as_ref()?;
    let label = words(profile.category).book_label;
    let kind = match action.kind {
        ActionKind::Link => BookKind::Page,
        ActionKind::Contact if action.value.starts_with("tel:") => BookKind::Call,
        ActionKind::Contact => BookKind::Email,
        // Refused by the profile's own check; never shown.
        ActionKind::Live => return None,
    };
    Some(BookView {
        label,
        url: action.value.clone(),
        kind,
    })
}

/// The view of `profile`, with its images in `folder`.
pub fn business(profile: &Profile, folder: &Path) -> BusinessView {
    let image = |f: &Option<Field<String>>| {
        f.as_ref()
            .map(|f| folder.join(&f.value).to_string_lossy().into_owned())
    };
    let colour = |f: &Option<Field<String>>| f.as_ref().map(|f| f.value.clone());
    let mut hidden_prices = 0;
    let services = profile
        .services
        .iter()
        .map(|s| {
            let price = s
                .shown_price()
                .map(|p| format_price(&p.amount, &p.currency));
            if s.price.is_some() && price.is_none() {
                hidden_prices += 1;
            }
            ServiceView {
                name: s.name.value.clone(),
                price,
                duration: s
                    .duration_minutes
                    .as_ref()
                    .map(|d| format_duration(d.value)),
                notes: s.notes.as_ref().map(|n| n.value.clone()),
            }
        })
        .collect();
    BusinessView {
        id: profile.id.clone(),
        name: profile.name.value.clone(),
        tagline: profile.tagline.as_ref().map(|f| f.value.clone()),
        description: profile.description.as_ref().map(|f| f.value.clone()),
        category: category_name(profile.category).into(),
        primary_color: colour(&profile.branding.primary),
        accent_color: colour(&profile.branding.accent),
        brand: profile
            .branding
            .primary
            .as_ref()
            .or(profile.branding.accent.as_ref())
            .and_then(|f| parse_hex(&f.value))
            .map(brand_colors),
        logo_path: image(&profile.branding.logo),
        hero_path: image(&profile.branding.hero),
        words: words(profile.category),
        services,
        hidden_prices,
        hours: hours(&profile.hours),
        contact: contact(profile),
        book: book(profile),
        concept_label: concept_label(profile),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_colours_read_in_light_and_dark() {
        // Near-black, a pale gold and a mid blue: each made readable both ways.
        for colour in [0x171717, 0xC5A66A, 0x3B82F6, 0xFFFFFF, 0x000000] {
            let b = brand_colors(colour);
            assert!(
                contrast(b.light_text, LIGHT_BACKGROUND) >= AA,
                "{colour:06x}"
            );
            assert!(contrast(b.dark_text, DARK_BACKGROUND) >= AA, "{colour:06x}");
            assert!(
                contrast(b.light_solid, b.light_on_solid) >= AA,
                "{colour:06x}"
            );
            assert!(
                contrast(b.dark_solid, b.dark_on_solid) >= AA,
                "{colour:06x}"
            );
        }
        // A colour that already reads is kept as it is.
        assert_eq!(brand_colors(0x171717).light_text, 0x171717);
        assert_eq!(brand_colors(0x171717).light_solid, 0x171717);
    }

    #[test]
    fn prices_durations_and_numbers() {
        assert_eq!(format_price("45.00", "GBP"), "£45.00");
        assert_eq!(format_price("30", "CHF"), "30 CHF");
        assert_eq!(format_duration(90), "1 hr 30 min");
        assert_eq!(format_duration(60), "1 hr");
        assert_eq!(
            tel_url("+44 (0)1506 000 000").as_deref(),
            Some("tel:+4401506000000")
        );
        assert_eq!(tel_url("call us"), None);
        assert_eq!(parse_hex("#C5A66A"), Some(0xC5A66A));
        assert_eq!(parse_hex("C5A66A"), None);
    }
}
