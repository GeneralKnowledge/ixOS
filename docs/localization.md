# Localization

Forge apps are localized with Apple's String Catalogs (`.xcstrings`), the file Xcode edits. Xcode compiles a catalog at build time with a Mac-only tool, so on Linux `forge` compiles it instead: `forge config sync` (and every `forge build ios` / `forge run ios`) writes `Generated/Localization/<language>.lproj/` and lists those folders in `xtool.yml`, which copies them into the app. At run time iOS reads them exactly as it reads an Xcode build's.

## Set it up

1. List the languages, development language first, and the catalog in `forge.toml`:

   ```toml
   [ios]
   languages = ["en", "es"]
   string_catalogs = ["Resources/Localizable.xcstrings"]
   ```

   `languages` also writes `CFBundleDevelopmentRegion` and `CFBundleLocalizations`, so iOS offers the app's language in Settings.

2. Write SwiftUI as usual. A literal in `Text`, `Button`, `Label`, `Toggle`, `Stepper` or `String(localized:)` is a key; interpolations become format specifiers, so `Text("\(count) books")` is the key `%lld books`.

3. Add each key to the catalog with its translations. The catalog is JSON, so any editor works; on a Mac, Xcode finds the keys for you and you can commit what it writes.

   ```json
   {
     "sourceLanguage" : "en",
     "version" : "1.0",
     "strings" : {
       "Settings" : {
         "localizations" : {
           "es" : { "stringUnit" : { "state" : "translated", "value" : "Ajustes" } }
         }
       },
       "%lld books" : {
         "localizations" : {
           "en" : { "variations" : { "plural" : {
             "one" : { "stringUnit" : { "state" : "translated", "value" : "%lld book" } },
             "other" : { "stringUnit" : { "state" : "translated", "value" : "%lld books" } }
           } } },
           "es" : { "variations" : { "plural" : {
             "one" : { "stringUnit" : { "state" : "translated", "value" : "%lld libro" } },
             "other" : { "stringUnit" : { "state" : "translated", "value" : "%lld libros" } }
           } } }
         }
       }
     }
   }
   ```

4. Run `forge config sync --project <app>` and commit `Generated/Localization/` with the catalog. `forge config check` (and CI) fails when the compiled files are out of date.

Sync warns about strings with no translation in a language (they show in the development language) and about languages the catalog has but `forge.toml` doesn't list.

## Permission texts and the app name

Name a second catalog `InfoPlist.xcstrings` and use Info.plist keys as its keys (`NSCameraUsageDescription`, `CFBundleDisplayName`). It compiles to `InfoPlist.strings`, which iOS uses for the permission prompts and the name under the icon.

## What it covers

- Plain strings, plural variations, and substitutions (two plurals in one string). Device variations are refused with a message.
- Strings marked "Don't translate" are left out.
- Tables are named after the catalog file: `Localizable.xcstrings` is the default table; `Settings.xcstrings` is read with `Text("…", tableName: "Settings")`.

## Not covered yet

- Finding keys in Swift source on Linux. Add them by hand, or let Xcode do it on a Mac.
- Text made in Rust, such as error messages, is English. Keep user-facing wording in Swift where it can be localized, or have Rust return a code that Swift turns into text.
- ForgeAppKit's built-in buttons (OK, Try Again) pass plain `String`s and stay English.
- Right-to-left layout works as in any SwiftUI app, but no Forge app has been checked in Arabic or Hebrew.

## Checking it on an iPhone

The services check app ([native-services-device-check.md](native-services-device-check.md), step 7) is in English and Spanish. Its Languages section shows the languages iOS sees and reads the Spanish table directly, so it proves the files reached the bundle whatever the phone's language is.
