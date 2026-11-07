# Kataster on iPhone (trial)

This is a compatibility trial: running the open-source Bevy game [Kataster](https://github.com/Bobox214/Kataster) by Boris Boutillier on an iPhone, built on Linux. The plan and findings are in [docs/bevy-game-trial.md](../../docs/bevy-game-trial.md).

The game is not copied into this repository. `fetch.sh` clones it at a pinned commit and applies [`patch/0001-ixos-iphone-trial.patch`](patch/0001-ixos-iphone-trial.patch). The patch adds one iOS-only file, `src/mobile.rs`, which provides touch controls, screen fit, pause on suspend and a frame-rate log. It also adds two lines to `main.rs` and the `virtual_joystick` dependency.

Kataster's code is MIT OR Apache-2.0. Its assets are Kenney's (CC0) and Fira Sans (SIL OFL 1.1, `assets/Fira-LICENSE`), and its credits screen names both.

## Run it

1. Set up xtool and the Darwin SDK: `xtool setup`. bindgen also needs libclang: `sudo apt install libclang-dev`.
2. Fetch the game and apply the patch: `./fetch.sh`
3. Build the game and assemble an unsigned `Kataster.app`: `./build.sh`. The first build takes about 9 minutes.
4. Sign the app with your Apple ID and install it over USB: `xtool install Kataster.app`. This uses one of a free account's three app slots.
5. Read the game's warnings and its `[trial] fps` lines: `forge device logs --process kataster --grep "[trial]"`.

## Controls

- **Left half of the screen:** a joystick. Push up to thrust, and left or right to turn.
- **Right half:** hold to fire.
- **Top-right corner:** tap to pause or resume.
- **Menus:** tap an entry to choose it. **Exit** quits the app.
