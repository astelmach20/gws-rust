---
"gws-rust": patch
---

`gmail +watch` and `events +subscribe` no longer miss a Ctrl+C or SIGTERM that arrives between pull iterations. The signal was only delivered to loops already waiting at that instant, so one received while the loop was handling a response was lost and every later Ctrl+C/SIGTERM was swallowed too, leaving the stream running until killed. The shutdown request is now latched, so the loop stops at its next wait.
