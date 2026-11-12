---
area: Sign-in
stage: implemented
---

# The session endpoint answers to the browser's cookie

Used by enocone's own suite.

## Given

1. **Alice signs in through the browser**
   ```browser
   - goto: "{{env.WEB}}/login"
   - fill: { label: Username or email, value: alice }
   - fill: { label: Password, value: secret }
   - click: Sign In
   - expect: { heading: Dashboard }
   ```

## When

2. **The session endpoint is asked with the browser's cookies**
   ```http
   GET: "{{env.WEB}}/api/auth/session"
   session: browser
   expect:
     status: 200
     json:
       $.user.name: alice
   capture: { access_token: $.accessToken }
   ```

## Then

3. **Without the browser's cookies, the same endpoint answers as signed out**
   ```http
   GET: "{{env.WEB}}/api/auth/session"
   expect:
     json:
       $.accessToken: { exists: false }
   ```
