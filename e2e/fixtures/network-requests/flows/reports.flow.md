---
area: Reports
stage: implemented
---

# The reports page calls only the APIs it should

Used by enocone's own suite.

## When

1. **Someone opens the reports page and exports it**
   ```browser
   - goto: "{{env.WEB}}/reports"
   - expect: { text: "Total: 42" }
   - click: Export
   - expect: { text: Exported 3 rows }
   - requests:
       - url: "*/api/quarterly/*"
         expect: { count: ">=1", status: 200 }
       - url: "*/api/admin/*"
         expect: { count: 0 }
       - url: "*/api/export*"
         method: GET
         expect: { urls: "*/api/export/scoped/*" }
   ```
