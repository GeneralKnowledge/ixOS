# Tools

What developers use to build, test and inspect an app: the `forge` command, templates, device access and UI tests. Everything here works for a person with no AI agent. Which code belongs here is in [areas.md](areas.md).

## Start here

| Page | What it's for |
|---|---|
| [getting-started.md](getting-started.md) | From a bare Linux machine to your first app on your iPhone, about an hour |
| [first-app.md](first-app.md) | Add a feature to the minimal starter app, about 20 minutes |
| [human-workflow.md](human-workflow.md) | The whole loop with the CLI alone, from `forge init` to reviewing evidence |
| [troubleshooting.md](troubleshooting.md) | A build, install or launch failed, or the app opens to a blank screen |
| [linux-setup.md](linux-setup.md) | Each toolchain step by hand, if `forge setup ios` couldn't do one |

## Making and changing apps

| Page | What it's for |
|---|---|
| [project-generation.md](project-generation.md) | What `forge init` creates, with and without `--minimal` |
| [feature-recipe.md](feature-recipe.md) | Every piece of one whole feature, layer by layer: the specification for the planned feature template |
| [json-output.md](json-output.md) | `--json`: the result object every command prints, and its exit codes |

## Testing on the phone

| Page | What it's for |
|---|---|
| [forge-device.md](forge-device.md) | Your iPhone from Linux: details, live logs, crash reports |
| [forge-ui-testing.md](forge-ui-testing.md) | UI tests that tap and type on a real iPhone (`forge ui`) |
| [visual-regression.md](visual-regression.md) | Noticing when a change made a screen look worse |
| [accessibility-report.md](accessibility-report.md) | Likely accessibility problems in the Swift sources, found on Linux before the phone (report only) |
| [evidence.md](evidence.md) | Recording what a run proved |
| [device-validation.md](device-validation.md) | The per-app phone check: compiles, installs, launches, main interactions |
| [native-services-device-check.md](native-services-device-check.md), [local-ai-check.md](local-ai-check.md) | Runbooks for the generated check apps |
| [android-setup.md](android-setup.md) | Android, which is work in progress |

Agent tooling (the MCP server, the explorer, run records, the live dev connection) is the separate [forge-lab](https://github.com/GeneralKnowledge/forge-lab) project; [AGENTS.md](../AGENTS.md) has the rules an agent follows here.
