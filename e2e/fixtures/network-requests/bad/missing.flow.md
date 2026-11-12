---
area: Reports
stage: implemented
---

# A request the page never made

Used by enocone's own suite.

## When

1. **The reports page is expected to call the audit API**
   ```browser
   - goto: "{{env.WEB}}/reports"
   - expect: { text: "Total: 42" }
   - requests:
       - url: "*/api/audit/*"
         expect: { count: ">=1" }
   ```
