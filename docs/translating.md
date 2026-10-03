# Translating Torqa

Torqa is written in English and translated with gettext files in `app/translations/`
(R24). German (`de.po`, Swiss spelling) is the first translation.

## For riders

Pick the language in **Rider settings → Profile → Language**: *System language* follows the
computer, or choose a language explicitly. Each rider has their own. Some texts switch on the
next screen change.

## Adding or updating a language

1. In the dev container, refresh the template after code changes:
   `python3 scripts/i18n/extract.py` → `app/translations/torqa.pot`.
2. Copy `torqa.pot` to `<code>.po` (e.g. `fr.po`), set `"Language: fr\n"` in the header and
   fill in every `msgstr`. Keep placeholders like `%s`, `%d`, `%.1f` and `%%` — the check fails
   if they differ.
3. Add the file to `internationalization/locale/translations` in `app/project.godot` and the
   language to `ProfileDialog.LANGUAGES`.
4. `scripts/check.sh` runs `extract.py --check`: the template must be current and every
   translation complete.

## For developers

- Wrap user-facing text in `tr("…")` (or `TranslationServer.translate("…")` in static
  functions) *before* formatting: `tr("Saved %s") % name`.
- Texts in scenes (`text`, `tooltip_text`, `title`, …) are found automatically.
- Lists of names (zones, weather, figures sent from the core) go between `i18n-begin` and
  `i18n-end` comments — in GDScript and in Rust; the front end translates them when shown.
- Texts built at run time go into `app/translations/extra-msgids.txt`.
- Names of people, routes, courses and devices are never translated: switch automatic
  translation off on controls that show them.
