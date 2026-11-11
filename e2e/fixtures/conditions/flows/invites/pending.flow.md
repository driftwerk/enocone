---
area: Invites
stage: implemented
---

# Pending people are invited again

Used by enocone's own suite: only the people still pending get an invite.

## Given

1. **The app has three people: two Pending and one Active**
   ```http
   GET: "{{env.API}}/people/g{{run.short_id}}"
   expect:
     json:
       "$[?@.name == 'ana'].status": Pending
       "$[?@.name == 'ben'].status": Active
       "$[?@.name == 'cy'].status": Pending
   capture: { ben_status: "$[?@.name == 'ben'].status" }
   ```

## When

2. **Each pending person is invited**

   Only if: {{row.status}} == "Pending"

   | name | status  |
   |------|---------|
   | ana  | Pending |
   | ben  | Active  |
   | cy   | Pending |

   ```http
   POST: "{{env.API}}/invites/g{{run.short_id}}"
   json: { name: "{{row.name}}" }
   expect: { status: 201 }
   ```

3. **Ben is reminded, if he is still pending**
   Only if: {{ben_status}} in ["Pending", "Invited"] and not {{ben_status}} == "Active"
   ```http
   POST: "{{env.API}}/invites/g{{run.short_id}}"
   json: { name: ben }
   ```

## Then

4. **Only Ana and Cy were invited**
   ```http
   GET: "{{env.API}}/invites/g{{run.short_id}}"
   expect:
     json:
       $.invited: [ana, cy]
   ```
