# ForgeUI showcase

Every [ForgeUI](../../docs/forgeui.md) component, live, and three complete screens built only from ForgeUI and SwiftUI.

- **Components** tab: Foundations (colour with a live contrast check, spacing and corners, type), Controls (buttons, icon buttons, text fields, form fields), Content (cards, rows, badges and status, headers) and Feedback (empty, loading and error states; alerts, dialogs and sheets). Each page shows the component, options that change it, and the code that makes it.
- **Screens** tab: a settings screen, a task list (search, a summary card, badges, swipe actions with confirmation, and loading, failed, empty and no-results states from the Simulate menu), a task detail screen, and a create/edit form in a sheet with validation messages and a busy Save.
- The paint palette in every toolbar switches the theme (Standard or Forest), light or dark, and the text size, so every component can be seen in every combination.

It is a plain SwiftUI app: no Rust core, no generated bindings, no `forge.toml`. That is deliberate. ForgeUI has to be useful in any SwiftUI app, and the showcase proves the components; [Pocket Library](../pocket-library/README.md) will prove the whole framework.

## Build and run

On Linux with xtool and an iPhone connected (Developer Mode on):

```bash
cd examples/forgeui-showcase
xtool dev           # build, sign with your Apple ID, install
```

`xtool dev build` builds without the phone. The bundle id is `org.forgemobile.agentlab`, the sandbox id agent-made apps share, so it replaces whatever app last used it.

The models (task list, form rules, the two themes) import only Foundation and Observation, so `swift test` runs their tests on Linux too.
