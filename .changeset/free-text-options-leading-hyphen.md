---
"gws-rust": patch
---

Helper options that take free text accept a value that starts with `-`. Before, `docs`/`sheets`/`slides +create --title '-draft'`, `sheets +create --sheet`, `gmail +search --query '-in:spam'`, `gmail +send --subject`/`--body`, `gmail +filter create --from`/`--to`/`--subject`/`--query`/`--exclude`, `calendar +insert`/`+update --summary`/`--location`/`--description`, `calendar +agenda --calendar-name`, `calendar +rsvp --comment`, `chat +send --text`, `tasks +add --title`/`--notes`, `drive +upload --name`, `drive +share --message`, `people +find --query`, `admin +user-create --given-name`/`--family-name`, `modelarmor +sanitize-prompt`/`+sanitize-response --text` and `workflow +file-announce --message` rejected such a value as an unknown flag.
