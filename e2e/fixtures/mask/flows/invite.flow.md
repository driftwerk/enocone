---
area: Invites
stage: implemented
---

# Jane opens her invite link

Used by enocone's own suite: the invite link is marked secret. The last step fails on
purpose, so the run records a page snapshot.

## Given

1. **Jane is sent a quote with an invite link**
   ```http
   POST: "{{env.API}}/quotes"
   headers: { X-API-Key: agency-key-98765 }
   json: { lead_id: ld-1, client_email: "jane+mask{{run.short_id}}@example.test", fees: [{ amount: 1000, vat: false }] }
   expect: { status: 201 }
   ```
   ```email
   to: "jane+mask{{run.short_id}}@example.test"
   subject: Your conveyancing quote
   within: 10s
   capture: { invite_link: { link: "/accept/", secret: true } }
   ```

## Then

2. **The invite link welcomes Jane back**
   ```browser
   - goto: "{{invite_link}}"
   - expect: { heading: Welcome back, timeout: 1s }
   ```
