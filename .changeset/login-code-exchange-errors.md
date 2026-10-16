---
"gws-rust": patch
---

`gwsr auth login`: when Google rejects the authorization code (`invalid_grant`, for example a code that expired or was already used), the error now says so. It used to claim that a stored refresh token had expired or been revoked. This error, the "no refresh token" error and a refused-scope error now name the profile being signed in to (`gwsr auth login --profile work`) instead of the default profile.
