---
area: Invites
stage: implemented
---

# A condition that can't be read

Used by enocone's own suite.

## When

1. **Each pending person is invited**

   Only if: {{row.status}} = Pending

   | name | status  |
   |------|---------|
   | ana  | Pending |

   ```http
   POST: "{{env.API}}/invites/bad"
   json: { name: "{{row.name}}" }
   ```

2. **A condition on a column the table doesn't have**

   Only if: {{row.state}} == "Pending"

   | name | status  |
   |------|---------|
   | ana  | Pending |

   ```http
   POST: "{{env.API}}/invites/bad"
   json: { name: "{{row.name}}" }
   ```
