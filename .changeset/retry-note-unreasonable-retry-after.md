---
"gws-rust": patch
---

An API error whose `Retry-After` asks for more than a day no longer quotes a meaningless number of seconds. A header too large to represent used to print as "server asked to retry after 18446744073709551615s"; the note now says "server asked to retry after an unreasonably long wait (over 86400s)". Waits of up to a day are still printed in seconds.
