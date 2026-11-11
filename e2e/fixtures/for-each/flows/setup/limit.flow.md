---
area: Setup
stage: implemented
---

# Refuse to delete more than expected

Used by enocone's own suite.

## Given

1. **The group has resources**
   ```http
   POST: "{{env.API}}/resources/limit{{run.short_id}}"
   json: { name: demo }
   expect: { status: 201 }
   ```
   ```http
   POST: "{{env.API}}/resources/limit{{run.short_id}}"
   json: { name: demo }
   expect: { status: 201 }
   ```
   ```http
   POST: "{{env.API}}/resources/limit{{run.short_id}}"
   json: { name: demo }
   expect: { status: 201 }
   ```
   ```http
   POST: "{{env.API}}/resources/limit{{run.short_id}}"
   json: { name: keep }
   expect: { status: 201 }
   ```

## When

2. **The ids of the demo resources are captured**
   ```http
   GET: "{{env.API}}/resources/limit{{run.short_id}}"
   capture: { old_ids: "$.results[?@.name == 'demo'].id" }
   ```

3. **Each old demo resource is deleted**

   For each: {{old_ids}} (at most 2)

   ```http
   DELETE: "{{env.API}}/resources/limit{{run.short_id}}/{{item}}"
   expect: { status: 204 }
   ```

## Cleanup

4. **Nothing was deleted: all four resources are still there**
   ```http
   GET: "{{env.API}}/resources/limit{{run.short_id}}"
   expect:
     json:
       $.results: { len: 4 }
   ```
