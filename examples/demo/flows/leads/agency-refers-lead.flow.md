---
area: Leads
tags: [smoke, agency]
---

# An agency referral without a property is rejected with a clear reason

Agencies integrate over the API. A referral missing the property must be
rejected up front, not half-created, so they can fix it and resend.

## When

1. **Harbour Estates sends a referral with no property**
   ```http
   POST: "{{env.API}}/leads"
   auth: agency-api-key
   json: { transaction: sale, clients: [{ name: "Sam Lee" }] }
   expect:
     status: 422
     json: { $.error: { contains: property } }
   ```

## Then

2. **Nothing can be fetched without credentials**
   ```http
   GET: "{{env.API}}/leads/does-not-matter"
   expect: { status: 401 }
   ```
