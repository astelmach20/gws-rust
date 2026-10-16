---
"gws-rust": patch
---

Auth commands no longer treat a stored file they cannot inspect (for example inside a profile directory left unsearchable by `sudo gwsr auth login`) as a missing one. Before, such a file was treated as absent. Now it is an error that names the path:

- `auth logout` fails before removing anything instead of skipping the revocation;
- `auth use` and `auth export` no longer say the profile has no credentials;
- `auth list` no longer reports `"logged_in": false`;
- a saved OAuth client file no longer falls back to another client;
- `auth status` reports `invalid_profiles_error`, `client.overrides_error` or `note_error` instead of leaving the field out.
