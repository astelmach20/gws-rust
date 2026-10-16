---
"gws-rust": patch
---
On Windows consoles that cannot render ANSI escape sequences (the classic console host without virtual terminal processing), errors and logs on stderr are now plain text instead of showing raw `←[1;31m` codes; gwsr turns virtual terminal processing on where the console supports it.
