---
area: Leads
tags: [lifecycle]
stage: approved
approved_by: product (demo)
---

# A case handler closes a lead as lost and can reopen it later

Clients come back. Closing a lead must not lose its history, and reopening
must put it back in the handler's list.

## Given

1. **A lead exists**
   ```http
   POST: "{{env.API}}/leads"
   auth: agency-api-key
   json: { transaction: sale, property: { address: "2 Mill Lane" } }
   capture: { lead_id: $.id }
   ```

## When

2. **The case handler closes the lead as lost**
   ```http
   POST: "{{env.API}}/leads/{{lead_id}}/close"
   auth: agency-api-key
   expect: { json: { $.status: closed } }
   ```

3. **Later, the case handler reopens it**

## Then

4. **The lead is open again with its history intact**
