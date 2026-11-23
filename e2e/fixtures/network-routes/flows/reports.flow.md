---
area: Reports
stage: implemented
---

# The reports page shows the routed data, and its export never leaves the browser

Used by enocone's own suite.

## When

1. **Someone opens the reports page and exports it**
   ```browser
   - goto: "{{env.WEB}}/reports"
   - expect: { text: "Total: 7" }
   - click: Export
   - requests:
       - url: "*/api/quarterly/*"
         expect: { count: 1, status: 200 }
       - url: "*/api/export*"
         expect: { count: 1 }
   ```
