---
area: Fixture
---

# A run outlasts its access token

Used by enocone's own suite to check that browser-profile tokens are refreshed.

## When

1. **The API is called as the short-token user**
   ```http
   GET: "{{env.API}}/short/whoami"
   auth: short
   expect: { status: 200 }
   capture: { first: $.token_serial }
   ```

2. **Three seconds pass, longer than a token lasts**
   ```exec
   # Wait three seconds
   run: sleep 3
   ```

3. **The API is called again as the same user**
   ```http
   GET: "{{env.API}}/short/whoami"
   auth: short
   expect: { status: 200, json: { $.token_serial: { not: "{{first}}" } } }
   capture: { second: $.token_serial }
   ```
