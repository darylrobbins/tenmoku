---
task_uid: "01m646pxqpb5gnv9gnkces8wm1"
task: "T1236"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test chat_models::default_is_listed: fe_config::chat_models::KNOWN contains Settings::default().chat.model ('claude-opus-5-5') and has no duplicates"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test settings_model_dynamic_choices::model_and_fonts_are_choices: chat.model has kind Choice(ChoiceSource::ChatModels) and each font_family key (editor, terminal, chat: the entries the schema has) has Choice(ChoiceSource::Fonts)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test settings_editor::choices_font_tests::model_dropdown_keeps_a_custom_id: with chat.model = 'my-proxy-model' in the file, the dropdown label is that id and the list ends with 'Other…' which turns the row into the text input (setting-input-chat.model) for typing a different id"
    required: true
    manual: false
  - id: "C4"
    text: "settings_editor::choices_font_tests::font_dropdown_lists_installed_fonts: the font_family dropdown lists the text system's all_font_names() sorted and deduplicated, first option 'Default' resets the key; the list is not rebuilt on every render (resolved once per open dropdown)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-config -p fe-workbench settings pass"
    required: true
    manual: false
---

# Offer a dropdown for chat.model (known Claude models, other ids kept) and for editor, terminal and chat font_family (installed fonts) in the Settings tab

## Problem
`chat.model` ("claude-opus-5-5", settings.rs ChatSettings) is a string with minLength 1, so it renders as a text box (SettingKind::Text) while `chat.effort` (enum ChatEffort) is a dropdown. font_family keys are Option<String> and also boxes. Typing a model id by hand is error prone.
## Change
Use the ChoiceSource seam from the themes card: add `ChatModels` (a curated list in a new fe-config chat_models.rs: the ids the app knows, default first; no network call) and `Fonts` (resolved in settings_editor_choices.rs from the GPUI text system's font names). Because new model ids ship before the app does, a model dropdown must never block a custom id: value not in the list shows as-is and an 'Other…' option swaps the row to the text input. Decided by the user (10-29): a curated static list, no network fetch.

## Implementation notes
- `chat_models::KNOWN` is opus 5.5 (the default, first), sonnet 5.5, haiku 5.5, fable 5.1.
- `choice_source` maps `chat.model` to `ChatModels` and any key ending in `font_family` to `Fonts`; the schema has `editor.font_family` and `terminal.font_family` (chat has none).
- A model or font not listed shows as-is with no "not installed" hint (`choices::takes_any`), so a closed font dropdown never calls `all_font_names()`; only opening one does (it's kept in the open `Select`). GPUI's `TextSystem::all_font_names` sorts and dedups.
- Other… is a flag on the open `Select`, after the options (`setting-option-other-<key>`); it's highlighted on opening when the value isn't listed. Picking it puts the key in `SettingsEditor::typing`, which draws the row as its input, focused; the input's blur commits and drops the key, unless the commit was refused (an empty id), so the message stays visible.
- `controls_tests::text_commit` used `editor.font_family` as its text box; it now uses `vim.leader`.
