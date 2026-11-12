---
area: Sign-in
stage: implemented
---

# The session endpoint is asked before any browser has signed in

Used by enocone's own suite.

## When

1. **The session endpoint is asked with the browser's cookies**
   ```http
   GET: "{{env.WEB}}/api/auth/session"
   session: browser
   ```
