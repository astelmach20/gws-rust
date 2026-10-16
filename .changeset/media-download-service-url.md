---
"gws-rust": patch
---

A method whose Discovery document sets `useMediaDownloadService` (such as Artifact Registry `files download`, Cloud Support `media download` or Drive `files get`) now sends an `alt=media` request to the download service URL (`<rootUrl>download/<servicePath>…`), as Google's generated client libraries do. Requests without `alt=media` keep the regular URL.
