---
"gws-rust": patch
---

On macOS and Windows, naming an API with different letter case (`gwsr Drive:v3`, `gwsr youtubeanalytics:v2`) no longer deletes the cached Discovery document of the real API (`drive`, `youtubeAnalytics`) and warns that it is an invalid cache entry. On a case-insensitive file system both names share one cache file, and that file is now treated as a cache miss.
