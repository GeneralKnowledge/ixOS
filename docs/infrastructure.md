# Infrastructure

What runs this repository: builds and the build queue, signing, the toolchain, CI and releases. Which files belong here is in [areas.md](areas.md).

| Page | What it's for |
|---|---|
| [builds.md](builds.md) | What each kind of build produces, the shared build queue, and releasing Forge itself |
| [ios-build-check.md](ios-build-check.md) | Checking that every app's Swift builds for an iPhone, on Linux with xtool |
| [ios-signing.md](ios-signing.md) | Apple sign-in, free and paid teams, the 7-day and 3-app limits |
| [compatibility.md](compatibility.md) | The tool versions Forge is built and tested with |
| [toolchain-validation.md](toolchain-validation.md) | How those versions were checked, step by step |
| [ios-xcode.md](ios-xcode.md) | Building with Xcode on a Mac, if you have one (optional) |
| [over-the-air.md](over-the-air.md) | Parked: why installs from the Install page don't work with a free Apple team |
| [first-app-store-release.md](first-app-store-release.md) | The plan for publishing to the App Store (not supported yet) |

CI is in `.github/workflows/`; which jobs gate what is in [CONTRIBUTING.md](../CONTRIBUTING.md#ci).
