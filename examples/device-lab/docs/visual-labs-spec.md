# Visual Labs: graphics in Python Lab, image generation in AI Lab

Status: **implementation spec, not started** (Reuben, 2026-11-21). Two tracks that make Device Lab's labs visual, and how Bonsai Harness operates them later. It doesn't change the [Bonsai Harness plan](bonsai-harness-plan.md); it builds on it.

## The shape

```
 Bonsai Harness       plans, tools, task state, verification, recovery   (coordinates; owns no capability)
   │ Environment trait: Python Lab first, AI Lab's images later
   ├── Python Lab     code, execution, and graphics Python itself makes (PNG, SVG, Matplotlib)
   └── AI Lab         text generation (#356) and image generation (new)
 Project and artifact storage: code, plots, SVGs, generated images, task evidence
```

**The Labs own their capabilities and screens; Bonsai only coordinates them.** Each Lab is useful on its own, with no agent. Bonsai reaches a capability only through the existing `Environment` trait and its tools, never through a second orchestration system.

## Where things stand (checked 2026-11-21)

| | What exists | Where |
|---|---|---|
| Python | CPython 3.15.0 (python.org iOS build), standard library trimmed, 53 extension modules wrapped as frameworks | `scripts/prepare-python.sh`, `Sources/PythonBridge`, `PythonRuntime.swift` |
| Python Lab screen | An editor and an output pane. **No project folder:** a manual run has no working directory of its own, so a script that saves `plot.png` has nowhere sensible to put it | `PythonLabModel.swift`, `PythonLabView.swift` |
| Projects | Only the harness has them: `Application Support/Python Lab/Projects/<name>`, files listed by `PythonLab::files()`; binary files show as "(not text)" | `rust/src/harness/python_lab.rs`, `BonsaiHarness::files()` |
| Run wrapper | Into the project folder, project on `sys.path`, project modules forgotten, audit-hook guard, tracebacks cut to the project's frames | `python_lab.rs` (`wrapper`, `GUARD`) |
| Model downloads | `LlamaModelFile` catalogue, foreground `URLSession` download, free-space and memory checks | `AiLabLlama.swift`, `LlamaLabModel.swift` |
| AI Lab runs | Rust `AiLab`: one run at a time, Stop, timings, memory and thermal before and after, history | `rust/src/ai_lab.rs` |

### What the Python packaging check found

PyPI's newest releases, checked for iOS wheels:

| Package | iOS wheels on PyPI | For |
|---|---|---|
| Pillow 13.0.0 | yes: cp313, cp314 (arm64 device and simulator) | **not cp315** |
| kiwisolver 1.5.1 (Matplotlib needs it) | yes: cp313, cp314 | not cp315 |
| numpy 2.5.5 | **none** | |
| matplotlib 3.11.2 | **none** | |
| contourpy 1.4.0 (Matplotlib needs it) | **none** | |
| fonttools | pure Python (no iOS wheel needed) | |

BeeWare publishes iOS builds of numpy and others on its own index (`pypi.anaconda.org/beeware`); that host is blocked from this cloud session, so which packages and Python versions it covers is **unverified**. Two facts follow:

1. **Matplotlib on the phone is not a download today.** numpy, Matplotlib and contourpy have native code with no iOS build on PyPI, and Python Lab runs 3.15, for which not even Pillow has an iOS wheel yet. So Matplotlib is its own milestone with a build spike, and the first graphics milestone needs no third-party package at all.
2. **Native wheels go through the same treatment as the standard library:** every `.so` becomes `Frameworks/<name>.framework` with a `.fwork` pointer, signed by xtool. `prepare-python.sh` already does exactly this for the standard library; it grows a pinned package list, each wheel checked by SHA-256, as python.org's tarball is.

## Track 1: Python Lab graphics

Python makes the files; Python Lab shows them. No graphics engine of our own.

### P1. Projects and artifact previews (Reuben's milestone 3, High)

**Python Lab gets a project.** Manual runs use the same wrapper as the harness, in a project folder: by default the `scratch` project the harness already uses for the person's own tasks, so a file Bonsai made and a file the person made are in one place. A project picker comes with H7; P1 needs only the one folder.

- Rust (`rust/src/harness/python_lab.rs`, plus a small export): `python_lab_script(project, code) -> String` returns the wrapped script for a manual run (folder, `sys.path`, module purge; the guard on with the network **allowed**, since the person wrote the code). `PythonLabModel` runs that instead of the bare code. The output format and the 30 s limit stay as they are.
- Rust: `project_artifacts(project) -> Vec<ProjectArtifact>`: files ending `.png`, `.jpg`, `.jpeg`, `.gif`, `.svg`, with `kind`, `bytes`, `modified_ms`, and for PNG the width and height read from its `IHDR` header (Rust, no image library). Newest first.
- New evidence for the harness: `Evidence::Produced { path, kind, bytes, width, height }`, recorded by `run_python` for any artifact that is new or changed after the run. A new criterion `Criterion::ProducesImage { path, kind, min_width, min_height }` ("running it produces chart.png, at least 400 pixels wide").
- Swift, view files only:
  - `ArtifactsView.swift`: the project's artifacts as a list with thumbnails, newest first; tapping one opens it full screen.
  - PNG, JPEG, GIF: `UIImage(contentsOfFile:)` in an `Image`, zoom with `MagnifyGesture`, share with `ShareLink`.
  - SVG: no native SVG view exists in SwiftUI or UIKit, so a `WKWebView` (`UIViewRepresentable`) shows it, with the SVG inlined into an HTML page carrying `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; img-src data:` and JavaScript off (`WKWebpagePreferences.allowsContentJavaScript = false`). An SVG can't load anything from the network or run a script.
  - `PythonLabView` gets an **Artifacts** section under Output: what the last run made or changed, with a link to all of them. `BonsaiHarnessView`'s project section shows images as thumbnails instead of "(not text)".
- Samples in Python Lab's Examples menu, standard library only: **PNG gradient** (writes a PNG with `zlib` and `struct`), **SVG chart** (a bar chart as SVG text from a list of numbers).

Acceptance (phone, about 10 minutes):
- The PNG gradient sample makes `gradient.png`; it appears under Artifacts within a second of the run ending, with the right size; it opens, zooms and shares.
- The SVG chart sample's `chart.svg` shows bars and labels; an SVG that references `https://…` and contains `<script>` shows without loading or running either (a test file in the samples).
- Running again updates the thumbnail (changed file, not a stale cache).
- Harness: a new ladder task **T-P1** "Write chart.py that draws a bar chart of [3, 7, 5] as chart.svg using only the standard library, and run it", accepted by `ProducesImage`. The phone run is recorded passed or failed from the transcript and the file.

Linux tests: artifact listing and PNG header parsing (Rust); the two samples run under the test `python3` and produce valid files (Rust test reading back the PNG header and parsing the SVG as XML).

### P2. Matplotlib (Reuben's milestone 4, High)

Gated on a spike, because the packages don't exist for iOS 3.15 yet.

**Spike S-P2 (half a day, cloud):** settle where the native wheels come from, in this order:
1. BeeWare's index: do iOS wheels of numpy, contourpy, Matplotlib and Pillow exist for 3.14 or 3.15? (From a machine that can reach it.)
2. If only 3.14 exists: weigh moving Python Lab from 3.15 to 3.14, where Pillow and kiwisolver already have PyPI iOS wheels. Python Lab has no 3.15-only code, but #356's phone results were on 3.15.
3. If nothing exists: build them with `cibuildwheel` (which supports iOS) in a GitHub Actions macOS job, once per version bump, and pin the results by SHA-256. That is a build artifact fetched like python.org's tarball, not a Mac in anyone's workflow. Cross-building from Linux with xtool's SDK is the alternative to test; numpy's Meson build makes it harder.

Then the milestone:
- `prepare-python.sh` installs a pinned list (`numpy`, `matplotlib`, `contourpy`, `kiwisolver`, `pillow`, `cycler`, `pyparsing`, `python-dateutil`, `six`, `packaging`, `fonttools`) into the bundle's `python/lib/python3.x/site-packages`, wrapping each `.so` as a framework. App size grows by roughly 60 to 80 MB (to measure); frameworks don't use App IDs.
- `PythonBridge` start: `MPLBACKEND=Agg`, and `MPLCONFIGDIR` in the app's caches folder (Matplotlib writes its font cache there; the guard allows the temporary folder already, and this adds caches).
- Samples: **Matplotlib line chart** and **Matplotlib histogram**, saved with `plt.savefig("plot.png", dpi=150)`.
- The standard library check gains `numpy` and `matplotlib`.

Acceptance (phone): both samples produce a PNG within the 30 s limit (first import measured, since font cache building is slow); the plot shows axes, labels and text in Matplotlib's bundled DejaVu font; memory before and after recorded; the harness task **T-P2** "plot y = x² for x from −5 to 5 into square.png with Matplotlib" passes by `ProducesImage`.

If the spike finds no workable route, the fallback is pure-Python charting that needs no native code (for example pygal for SVG charts), recorded as a decision for Reuben, not taken silently.

**Study, ideas only:** Terrarium, for inline plots next to the code, tracebacks linked to the source line and editor UX. Its Pyodide runtime is not a reason to replace CPython.

## Track 2: AI Lab image generation

A capability of its own, beside text generation, with its own runtime interface; it reuses AI Lab's model downloads and screen conventions.

### I1. Research and prototype (Reuben's milestone 5, High, in parallel with P1 and P2)

**Runtimes**, read on 2026-11-21:

| Candidate | What it is | From Linux with xtool | On an 8 GB iPhone 15 Pro Max | Notes |
|---|---|---|---|---|
| **Mirage** (`haplollc/Mirage`, MIT) | Swift API over stable-diffusion.cpp and ggml-metal; runs anything sd.cpp loads (SD 1.5, SDXL-Turbo, SD3, Flux, Z-Image, Qwen-Image) | Likely: ships a **prebuilt XCFramework** as a release asset (a SwiftPM binary target, like llama.cpp); Metal source embedded and compiled at first load (+~8 s) | Its own table: 8 GB devices run SDXL-Turbo Q4 (~5 GB) or Z-Image-Turbo Q3 **with** `increased-memory-limit`; 6 GB devices SD 1.5 / SDXL-Turbo Q4. Tested on iPhone 17 Pro | **Two risks to test first:** (1) Device Lab already links llama.cpp's ggml; Mirage warns of ggml symbol clashes and hides its own with `ld -r`. (2) It asks for `increased-memory-limit` and `extended-virtual-addressing`; our [local-ai notes](../../../docs/local-ai.md) record the second as paid-only and the first as probably paid-only, so design for the **default** memory limit: SD 1.5 or SD-Turbo at 512² first |
| **Apple ml-stable-diffusion** (Core ML) | Apple's Swift package and pipeline; Neural Engine | Likely: plain Swift and Core ML; the models must be **already compiled** (`.mlmodelc`), which Apple publishes on Hugging Face (compiling needs macOS) | SD 1.5 / 2.1 at 512², with `reduceMemory` on iPhone | Fixed set of converted checkpoints; older models. A good comparison if Mirage clashes with llama.cpp |
| **MobileDiffuser** (MLX) | Pure Swift + MLX app; Z-Image Turbo 6B and FLUX.2 Klein 4B; validated on an **iPhone 16 Pro (8 GB)**: FLUX.2 Klein 512² in ~1m11s at ~4.3 GB peak, block streaming keeps 1024² at ~3.8 GB | Doubtful: MLX compiles its Metal kernels with Apple's `metal` compiler at build time, which xtool's Linux SDK doesn't have; an Xcode project with sibling repositories | The most memory-aware design here | **Study for ideas:** its memory governor, block-streaming partial loads, and latent previews while generating |
| **Apple Image Playground** (`ImageCreator`, iOS 18.4+) | Apple's on-device image model through an API | Likely (system framework) | No download; needs Apple Intelligence | Not on Reuben's list; worth one day because it proves the whole pipeline (run record, progress, Stop, saving, preview) before any model download. Only Apple's styles (animation, illustration, sketch); no seed, steps or negative prompt |

**No runtime is chosen yet.** The spike measures, on the phone, the same three numbers for each candidate it gets running: peak memory (`os_proc_available_memory` before, during and after), time for one 512² image, and whether it coexists with Bonsai loaded (it may have to unload Bonsai first; the harness journal makes that safe).

Spike order: (1) Image Playground, to build the pipeline; (2) Mirage with SD-Turbo or SD 1.5 Q8 at 512², checking the ggml clash in a Device Lab build that also has llama.cpp; (3) Core ML Stable Diffusion only if (2) clashes or doesn't fit.

**The interface** (whatever runtime wins):
- Rust, `rust/src/image_lab.rs`, the same shape as `ai_lab.rs`: `ImageLab` records each run (model, prompt, negative prompt, seed, steps, size, guidance, time to first preview, total time, memory and thermal before and after, the output path), keeps a history, one run at a time, Stop.
- A foreign trait implemented in Swift, `ImageGenerator`: `describe() -> ImageModelInfo` and `async generate(request: ImageRequest, progress: ImageProgress) -> Result<ImageResult, ImageModelError>`. `ImageProgress` carries step `n` of `m` and an optional preview file, and returns false to stop, like `TextSink`. **Pixels never cross the boundary:** Swift writes the PNG into the artifact folder and returns its path.
- Model downloads: `LlamaModelFile` and `LlamaLabModel` become one catalogue of downloadable lab models with a kind (text or image). This is the second use inside Device Lab, so the download code is shared then, not before; a diffusion model can be several files (model, VAE, text encoder).
- Screen: AI Lab ▸ **Images**: model, prompt, negative prompt, size, seed (with a dice button), steps; a progress bar with step count and preview; Stop; memory now; the result saved into the shared artifacts folder and shown with the same `ArtifactsView` as Python Lab.

Acceptance (phone): one 512² image from one prompt on the chosen runtime, with its run record; Stop part-way frees the memory (reading back within 10% of the before value); the same seed and settings give the same image twice; the result appears in Artifacts. Each number recorded in the plan's results, passed or failed, from the phone.

### I2. Image-to-image and generation controls (milestone 6, Later)

Image-to-image from an artifact (strength, or reference images where the runtime supports them), samplers, guidance, a gallery with each image's settings so a run can be repeated.

## Bonsai integration

The harness already has what this needs; nothing here is a second orchestrator.

- **Python graphics (with P1/P2):** `Evidence::Produced` and `Criterion::ProducesImage`. Bonsai is a text model and **can't see pixels**, so its "inspect" step uses facts the environment reports: the file exists, its type, its size, and for a chart the data it was drawn from (the code can also print what it plotted). The person judges how it looks. A small Python check can add measurable facts (Pillow's colour count, an SVG's element count) once Pillow exists.
- **Image generation for Bonsai (milestone 7, Later):** AI Lab's images become a second `Environment` (or a tool in Python Lab's) with one tool, `generate_image(prompt, path, size)`, behind an explicit per-project permission (off by default, like the network), a resource limit (one image per task, 512² by default) and the rule that Bonsai's model is unloaded first if memory needs it, resumed from the journal afterwards.
- **Shared artifact library (milestone 8, Later):** one storage for code, plots, SVGs, generated images and task evidence across the Labs, each artifact with where it came from (a run, a task, a generation with its settings).

## Order

| # | Milestone | Priority | Depends on |
|---|---|---|---|
| 1 | Merge #356 | Foundation | its combined build |
| 2 | Validate and merge #357 (Bonsai Harness first slice) | Foundation | the phone ladder T1 to T3 |
| 3 | **P1** projects and artifact previews | High | 2 (uses the harness wrapper and evidence) |
| 4 | **P2** Matplotlib | High | spike S-P2, then 3 |
| 5 | **I1** image generation research and prototype | High, in parallel | 1 only |
| 6 | I2 image-to-image and controls | Later | 5 |
| 7 | Bonsai with image generation | Later | 5 and the harness's H4 |
| 8 | Shared artifact library | Later | 3 and 5 |

Image generation stays ambitious but runs beside the harness work and never ahead of it: each spike is time-boxed, and a finding goes into this document before any runtime is adopted. No dependency enters ixOS from this spec until its spike has passed on the phone.
