# Your first app: add a feature

This tutorial takes about 20 minutes. You make the minimal starter app and add one feature to it, a "Clear Done" button, which touches every part of a Forge app once: a rule in Rust, a test, the Swift view model and a button on the screen. You need the machine set up ([getting-started.md](getting-started.md) steps 1 and 2). The iPhone is needed only for the last step.

## What you're building with

A Forge app has two halves:

- **Rust owns the app.** Its data, the rules for changing it, and saving it live in `rust/src/lib.rs`. You test them with `cargo test` in seconds, without a phone or the iOS SDK.
- **SwiftUI shows it.** The screens are in `Sources/AppHost/`. A small view model (`ItemsModel.swift`) calls Rust and holds what the screen shows.

Forge generates the Swift API for your Rust (`import ForgeFFI`) every time you build, so a Rust function marked `#[uniffi::export]` is a Swift method a moment later.

| I want to… | I edit |
|---|---|
| Change a rule (what is valid, what an action does) | `rust/src/lib.rs` |
| Store more data | `rust/src/lib.rs`: the record (`Item`) and what is saved (`Saved`) |
| Change how something looks | the view, `Sources/AppHost/ContentView.swift` |
| Make a button do something new | Rust method → `ItemsModel.swift` → a button in the view (this tutorial) |
| Add a screen | a new `View` file in `Sources/AppHost/`, opened with a `NavigationLink` or `.sheet` ([ios-ui.md](ios-ui.md)) |
| Use the camera, location, notifications… | a native service ([native-services.md](native-services.md)); start from the full starter |

## 1. Create the app

```bash
cd ~/projects
forge init Shopping --minimal --bundle-id com.yourname.shopping
cd Shopping
```

`--minimal` gives you a checklist app and nothing else: about 200 lines of Rust and three small Swift files. Without it, `forge init` creates the full starter, a notes app that shows events, networking, the app lifecycle and native services. Come back to that when you need one of them.

Run the Rust tests to see that everything works:

```bash
cargo test --manifest-path rust/Cargo.toml
```

## 2. Add the rule in Rust

Open `rust/src/lib.rs`. `AppCore` is the app; each `pub` method in its `#[uniffi::export]` block is something Swift can call. Add one after `delete_item`:

```rust
    /// Removes every ticked item. Returns the new list.
    pub async fn clear_done(&self) -> Result<Vec<Item>, AppError> {
        self.change(|saved| {
            saved.items.retain(|i| !i.done);
            Ok(())
        })
        .await
    }
```

`change` does the work every command shares: it applies your edit to a copy of the list, saves the copy to `items.json` on a background thread, and keeps it only if saving worked. The method is `async` because saving touches the disk; Swift will `await` it without freezing the screen.

Add a test at the bottom, inside `mod tests`:

```rust
    #[test]
    fn clear_done_keeps_the_open_items() {
        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir);
        block_on(core.add_item("Milk".into())).unwrap();
        let items = block_on(core.add_item("Bread".into())).unwrap();
        block_on(core.toggle_item(items[0].id)).unwrap();
        let items = block_on(core.clear_done()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Bread");
    }
```

```bash
cargo test --manifest-path rust/Cargo.toml
```

## 3. Call it from the view model

```bash
forge bindings generate     # optional: lets your editor see the new Swift method
```

Rust's `clear_done` is Swift's `clearDone()`. In `Sources/AppHost/ItemsModel.swift`, add next to `delete`:

```swift
    public func clearDone() async {
        await perform { core in try await core.clearDone() }
    }
```

`perform` shows the list Rust returns, or puts the error in `errorMessage` for the alert.

The view model imports no SwiftUI, so Forge can compile and test it on Linux with the plain Swift toolchain:

```bash
forge check swift
```

It builds your Rust for this computer, compiles `ItemsModel.swift` against the new bindings and runs the tests in `Tests/`. If you misspell `clearDone`, this is where you find out, in seconds rather than after an iOS build.

## 4. Add the button

In `Sources/AppHost/ContentView.swift`, give the list a toolbar. Add this after `.navigationTitle(...)`:

```swift
            .toolbar {
                Button("Clear Done") {
                    Task { await model.clearDone() }
                }
                .accessibilityIdentifier("items.clearDone")
                .disabled(!model.items.contains(where: \.done))
            }
```

Every control gets an accessibility identifier named `screen.element`. UI tests find it by that name (`forge ui`), and coding agents do too.

## 5. Run it on the iPhone

Connect the phone, unlock it, then:

```bash
forge run ios
```

It builds the Rust for iOS, regenerates the bindings, compiles the app, signs it and installs it, and ends with what to do next. Open Shopping on the phone, add a few items, tick some off and tap **Clear Done**. Close the app and open it again: the list is still there, because Rust saved it.

If it opens to a blank screen or closes at once, run `forge diagnose` and open the app when it asks: it checks the install, reads the newest crash report and the app's log, and says what to do next ([troubleshooting.md](troubleshooting.md#the-app-opens-to-a-blank-screen-or-closes-straight-away)).

## Next

- Store more: add a field to `Item` (say `quantity: u32`), use it in the view, and bump `SAVE_VERSION` if old files can't be read any more ([app-crate.md](app-crate.md)).
- Make it look like an iPhone app: [ios-ui.md](ios-ui.md) is the SwiftUI standard Forge apps follow.
- When you need events from Rust, networking, the app lifecycle or native services, read the full starter (`forge init MyApp` without `--minimal`) and [app-crate.md](app-crate.md).
- Publishing: Forge makes development builds today; the App Store plan is [first-app-store-release.md](first-app-store-release.md).
