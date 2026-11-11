//! Compiles String Catalogs (`.xcstrings`, the JSON file Xcode edits) into
//! the `.strings` and `.stringsdict` files iOS reads at run time, so an app
//! built from Linux with xtool is localized the same way an Xcode build is.
//!
//! `forge config sync` writes them to `Generated/Localization/<lang>.lproj/`
//! and xtool.yml lists each `.lproj` folder as a resource, which xtool copies
//! to the app bundle's root, where `Text("…")` and `String(localized:)` look.
//!
//! Supported: plain strings, plural variations, and substitutions (several
//! plurals in one string). Device variations are refused with a message.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde_json::Value;

/// Where the compiled files go, relative to the app.
pub const OUTPUT_DIR: &str = "Generated/Localization";

const PLURAL_FORMS: [&str; 6] = ["zero", "one", "two", "few", "many", "other"];

/// One catalog, compiled: per language, the `.strings` and `.stringsdict`
/// content (either may be absent when it would be empty).
#[derive(Debug, Default, PartialEq)]
pub struct Compiled {
    /// The table name: the catalog's file name without `.xcstrings`.
    pub table: String,
    pub source_language: String,
    /// language → (strings, stringsdict)
    pub languages: BTreeMap<String, (Option<String>, Option<String>)>,
    /// language → keys with no value in that language (source language excluded).
    pub missing: BTreeMap<String, Vec<String>>,
}

/// A string's value in one language.
enum Entry {
    Plain(String),
    /// The stringsdict entry: format key, then variables (name, value type, forms).
    Plural {
        format: String,
        variables: Vec<(String, String, BTreeMap<String, String>)>,
    },
}

/// Compiles one catalog. `languages` are the app's languages (forge.toml):
/// every one gets a table, even with no translations yet, so the app lists it.
pub fn compile(table: &str, json: &str, languages: &[String]) -> Result<Compiled, String> {
    let root: Value = serde_json::from_str(json)
        .map_err(|e| format!("{table}.xcstrings is not valid JSON: {e}"))?;
    let source = root
        .get("sourceLanguage")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{table}.xcstrings has no sourceLanguage"))?
        .to_string();
    let empty = serde_json::Map::new();
    let strings = match root.get("strings") {
        Some(Value::Object(map)) => map,
        None => &empty,
        Some(_) => return Err(format!("{table}.xcstrings: \"strings\" is not an object")),
    };

    let mut per_language: BTreeMap<String, Vec<(String, Entry)>> = BTreeMap::new();
    let mut missing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (key, string) in strings {
        if string.get("shouldTranslate").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let localizations = string.get("localizations").and_then(Value::as_object);
        for language in languages {
            let entry = match localizations.and_then(|l| l.get(language)) {
                Some(loc) => entry(loc)
                    .map_err(|e| format!("{table}.xcstrings \"{key}\" ({language}): {e}"))?,
                None => None,
            };
            match entry {
                Some(entry) => per_language
                    .entry(language.clone())
                    .or_default()
                    .push((key.clone(), entry)),
                // In the source language the key is the text.
                None if *language == source => {}
                None => missing
                    .entry(language.clone())
                    .or_default()
                    .push(key.clone()),
            }
        }
    }

    let mut compiled = Compiled {
        table: table.to_string(),
        source_language: source,
        languages: BTreeMap::new(),
        missing,
    };
    for language in languages {
        let entries = per_language.remove(language).unwrap_or_default();
        let mut strings = String::new();
        let mut plurals = Vec::new();
        for (key, entry) in &entries {
            match entry {
                Entry::Plain(value) => {
                    let _ = writeln!(strings, "\"{}\" = \"{}\";", escape(key), escape(value));
                }
                Entry::Plural { .. } => plurals.push((key, entry)),
            }
        }
        let stringsdict = (!plurals.is_empty()).then(|| render_stringsdict(&plurals));
        let strings = (!strings.is_empty()).then_some(strings);
        // An empty table still marks the language as one the app has.
        let strings = if strings.is_none() && stringsdict.is_none() {
            Some(String::new())
        } else {
            strings
        };
        compiled
            .languages
            .insert(language.clone(), (strings, stringsdict));
    }
    Ok(compiled)
}

/// Every language a catalog has strings for, so sync can warn about ones
/// forge.toml doesn't list.
pub fn catalog_languages(json: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return out;
    };
    if let Some(strings) = root.get("strings").and_then(Value::as_object) {
        for string in strings.values() {
            if let Some(l) = string.get("localizations").and_then(Value::as_object) {
                out.extend(l.keys().cloned());
            }
        }
    }
    out
}

fn entry(loc: &Value) -> Result<Option<Entry>, String> {
    if let Some(unit) = loc.get("stringUnit") {
        let value = unit_value(unit);
        let Some(value) = value else { return Ok(None) };
        let substitutions = loc.get("substitutions").and_then(Value::as_object);
        return match substitutions {
            None => Ok(Some(Entry::Plain(value))),
            Some(subs) => {
                let mut variables = Vec::new();
                for (name, sub) in subs {
                    let arg = sub.get("argNum").and_then(Value::as_u64).unwrap_or(1);
                    let spec = sub
                        .get("formatSpecifier")
                        .and_then(Value::as_str)
                        .unwrap_or("lld")
                        .to_string();
                    let forms = plural_forms(sub.get("variations"))?
                        .ok_or_else(|| format!("substitution {name} has no plural variation"))?;
                    // "%arg" in a substitution's forms is its argument.
                    let positional = format!("%{arg}${spec}");
                    let forms = forms
                        .into_iter()
                        .map(|(form, text)| (form, text.replace("%arg", &positional)))
                        .collect();
                    variables.push((name.clone(), spec, forms));
                }
                Ok(Some(Entry::Plural {
                    format: value,
                    variables,
                }))
            }
        };
    }
    if let Some(variations) = loc.get("variations") {
        if variations.get("device").is_some() {
            return Err(
                "device variations are not supported by Forge yet; use one text for every device"
                    .into(),
            );
        }
        let Some(forms) = plural_forms(Some(variations))? else {
            return Ok(None);
        };
        let spec = forms
            .get("other")
            .and_then(|t| first_specifier(t))
            .unwrap_or_else(|| "lld".into());
        return Ok(Some(Entry::Plural {
            format: "%#@value@".into(),
            variables: vec![("value".into(), spec, forms)],
        }));
    }
    Ok(None)
}

fn unit_value(unit: &Value) -> Option<String> {
    unit.get("value")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// The forms of a `plural` variation, or None when there is none.
fn plural_forms(variations: Option<&Value>) -> Result<Option<BTreeMap<String, String>>, String> {
    let Some(plural) = variations
        .and_then(|v| v.get("plural"))
        .and_then(Value::as_object)
    else {
        if variations.and_then(|v| v.get("device")).is_some() {
            return Err(
                "device variations are not supported by Forge yet; use one text for every device"
                    .into(),
            );
        }
        return Ok(None);
    };
    let mut forms = BTreeMap::new();
    for (form, value) in plural {
        if !PLURAL_FORMS.contains(&form.as_str()) {
            return Err(format!("unknown plural form \"{form}\""));
        }
        if let Some(text) = value.get("stringUnit").and_then(unit_value) {
            forms.insert(form.clone(), text);
        }
    }
    if !forms.contains_key("other") {
        return Err("a plural variation needs an \"other\" form".into());
    }
    Ok(Some(forms))
}

/// The conversion of the first format specifier in `text` ("lld" in
/// "%lld books", "@" in "%1$@").
fn first_specifier(text: &str) -> Option<String> {
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '%' {
            continue;
        }
        if chars.peek().map(|(_, c)| *c) == Some('%') {
            chars.next();
            continue;
        }
        let rest = &text[i + 1..];
        // Skip a position ("1$"), flags and width.
        let rest = match rest.find('$') {
            Some(d) if rest[..d].chars().all(|c| c.is_ascii_digit()) && d > 0 => &rest[d + 1..],
            _ => rest,
        };
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit() || "-+ #.0".contains(c));
        let len_mods: String = rest
            .chars()
            .take_while(|c| "hlqLzjt".contains(*c))
            .collect();
        let conv = rest[len_mods.len()..].chars().next()?;
        if conv.is_ascii_alphabetic() || conv == '@' {
            return Some(format!("{len_mods}{conv}"));
        }
    }
    None
}

/// Escapes for the old-style plist `.strings` format.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_stringsdict(plurals: &[(&String, &Entry)]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n",
    );
    for (key, entry) in plurals {
        let Entry::Plural { format, variables } = entry else {
            continue;
        };
        let _ = writeln!(out, "\t<key>{}</key>\n\t<dict>", xml(key));
        let _ = writeln!(
            out,
            "\t\t<key>NSStringLocalizedFormatKey</key>\n\t\t<string>{}</string>",
            xml(format)
        );
        for (name, spec, forms) in variables {
            let _ = writeln!(out, "\t\t<key>{}</key>\n\t\t<dict>", xml(name));
            let _ = writeln!(
                out,
                "\t\t\t<key>NSStringFormatSpecTypeKey</key>\n\t\t\t<string>NSStringPluralRuleType</string>"
            );
            let _ = writeln!(
                out,
                "\t\t\t<key>NSStringFormatValueTypeKey</key>\n\t\t\t<string>{}</string>",
                xml(spec)
            );
            for form in PLURAL_FORMS {
                if let Some(text) = forms.get(form) {
                    let _ = writeln!(
                        out,
                        "\t\t\t<key>{form}</key>\n\t\t\t<string>{}</string>",
                        xml(text)
                    );
                }
            }
            let _ = writeln!(out, "\t\t</dict>");
        }
        let _ = writeln!(out, "\t</dict>");
    }
    out.push_str("</dict>\n</plist>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = r#"{
      "sourceLanguage": "en",
      "version": "1.0",
      "strings": {
        "Settings": {
          "localizations": {
            "es": { "stringUnit": { "state": "translated", "value": "Ajustes" } }
          }
        },
        "Say \"hi\"\n": {
          "localizations": {
            "es": { "stringUnit": { "state": "translated", "value": "Di \"hola\"\n" } }
          }
        },
        "%lld books": {
          "localizations": {
            "en": { "variations": { "plural": {
              "one": { "stringUnit": { "state": "translated", "value": "%lld book" } },
              "other": { "stringUnit": { "state": "translated", "value": "%lld books" } }
            } } },
            "es": { "variations": { "plural": {
              "one": { "stringUnit": { "state": "translated", "value": "%lld libro" } },
              "other": { "stringUnit": { "state": "translated", "value": "%lld libros" } }
            } } }
          }
        },
        "%lld books in %lld shelves": {
          "localizations": {
            "es": {
              "stringUnit": { "state": "translated", "value": "%#@books@ en %#@shelves@" },
              "substitutions": {
                "books": { "argNum": 1, "formatSpecifier": "lld", "variations": { "plural": {
                  "one": { "stringUnit": { "state": "translated", "value": "%arg libro" } },
                  "other": { "stringUnit": { "state": "translated", "value": "%arg libros" } }
                } } },
                "shelves": { "argNum": 2, "formatSpecifier": "lld", "variations": { "plural": {
                  "one": { "stringUnit": { "state": "translated", "value": "%arg estante" } },
                  "other": { "stringUnit": { "state": "translated", "value": "%arg estantes" } }
                } } }
              }
            }
          }
        },
        "Untranslated": {},
        "Forge": { "shouldTranslate": false }
      }
    }"#;

    fn langs(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plain_strings_are_escaped() {
        let c = compile("Localizable", CATALOG, &langs(&["en", "es"])).unwrap();
        let (strings, _) = &c.languages["es"];
        let strings = strings.as_deref().unwrap();
        assert!(
            strings.contains("\"Settings\" = \"Ajustes\";\n"),
            "{strings}"
        );
        assert!(
            strings.contains("\"Say \\\"hi\\\"\\n\" = \"Di \\\"hola\\\"\\n\";\n"),
            "{strings}"
        );
        assert!(!strings.contains("Forge"));
    }

    #[test]
    fn plurals_become_a_stringsdict() {
        let c = compile("Localizable", CATALOG, &langs(&["en", "es"])).unwrap();
        let en = c.languages["en"].1.as_deref().unwrap();
        assert!(en.contains("<key>%lld books</key>"), "{en}");
        assert!(en.contains("<string>%#@value@</string>"), "{en}");
        assert!(
            en.contains("<key>NSStringFormatValueTypeKey</key>\n\t\t\t<string>lld</string>"),
            "{en}"
        );
        assert!(
            en.contains("<key>one</key>\n\t\t\t<string>%lld book</string>"),
            "{en}"
        );
        // English has no plain translations, but its plurals still go out.
        assert_eq!(c.languages["en"].0, None);

        let es = c.languages["es"].1.as_deref().unwrap();
        assert!(
            es.contains("<string>%#@books@ en %#@shelves@</string>"),
            "{es}"
        );
        assert!(es.contains("<string>%2$lld estantes</string>"), "{es}");
        assert!(es.contains("<string>%1$lld libro</string>"), "{es}");
    }

    #[test]
    fn reports_missing_translations_but_not_for_the_source_language() {
        let c = compile("Localizable", CATALOG, &langs(&["en", "es", "fr"])).unwrap();
        assert_eq!(c.missing["es"], vec!["Untranslated".to_string()]);
        assert_eq!(c.missing["fr"].len(), 5);
        assert!(!c.missing.contains_key("en"));
        // French has nothing yet but still gets a table.
        assert_eq!(c.languages["fr"], (Some(String::new()), None));
    }

    #[test]
    fn refuses_what_it_cannot_compile() {
        let device = r#"{"sourceLanguage":"en","strings":{"Tap":{"localizations":{"es":{"variations":{"device":{"iphone":{"stringUnit":{"value":"Toca"}}}}}}}}}"#;
        let err = compile("Localizable", device, &langs(&["es"])).unwrap_err();
        assert!(err.contains("device variations"), "{err}");
        let no_other = r#"{"sourceLanguage":"en","strings":{"%lld":{"localizations":{"es":{"variations":{"plural":{"one":{"stringUnit":{"value":"uno"}}}}}}}}}"#;
        let err = compile("Localizable", no_other, &langs(&["es"])).unwrap_err();
        assert!(err.contains("\"other\""), "{err}");
        assert!(compile("Localizable", "{", &langs(&["es"])).is_err());
    }

    #[test]
    fn finds_the_first_format_specifier() {
        assert_eq!(first_specifier("%lld books").as_deref(), Some("lld"));
        assert_eq!(first_specifier("100%% of %1$@").as_deref(), Some("@"));
        assert_eq!(first_specifier("%.1f km").as_deref(), Some("f"));
        assert_eq!(first_specifier("none"), None);
    }

    #[test]
    fn lists_the_catalog_languages() {
        let l = catalog_languages(CATALOG);
        assert_eq!(l.into_iter().collect::<Vec<_>>(), vec!["en", "es"]);
    }
}
