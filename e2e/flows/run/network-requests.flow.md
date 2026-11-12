---
area: Running flows
tags: [browser, network, expect]
owner: enocone
priority: normal
stage: approved
approved_by: Kingsley Hendrickse (2026-11-12)
---

# A browser step can check the requests the page made

Some behaviour shows only in the requests a page makes: "the export
downloads only from the scoped endpoints", or "this screen never calls the
admin API". A flow can't check that today. A browser step that checks the
page's requests can, without recording their bodies.

```flowchart
flowchart LR
  1[A reports page that loads its data and has an export button] --> 2[A flow opens it, exports, and checks the requests]
  2 --> 3[The data request is counted with status 200, and no admin request was made]
  2 --> 4[Every export request went to the scoped endpoint]
  2 -.->|a request the page never made| 5[Fails, listing the requests it did see]
  2 --> 6[The report lists the matched requests, without bodies or headers]
```

## Rules

- A browser block can check the page's requests with a `requests:` action,
  a list of patterns, each with what to expect:

  ```yaml
  - requests:
      - url: "*/api/reports/*"
        expect: { count: ">=1", status: 200 }
      - url: "*/api/admin/*"
        expect: { count: 0 }
      - url: "*/api/export*"
        method: GET
        expect: { urls: "*/api/export/scoped/*" }
  ```

- A pattern matches by `url`, where `*` stands for any characters, and
  optionally by `method`.
- `expect` can check:
  - `count`: a number, or a comparison such as `">=1"` or `"<3"`;
  - `status`: a status code, or a list of them, that every matching
    response must have;
  - `urls`: a pattern that every matching request's URL must match.
- The check covers the requests the page made since the step started, up
  to the `requests:` action. Before checking, it waits for requests still
  in flight to finish (up to the step's timeout), so a request a click
  just started is counted.
- `count: 0` passes only if no matching request was made.
- A failure names the pattern, what was expected, and the matching
  requests it saw (up to 10, with method, URL and status).
- The report lists the requests each pattern matched, with method, URL and
  status. Request and response bodies and headers aren't recorded. URLs are
  masked like other secrets.

## Given

1. **The demo app has a reports page that loads its data from the reports API, never calls the admin API, and has an export button that downloads from a scoped endpoint**

## When

2. **A flow opens the reports page, clicks Export, and checks the requests the page made**

## Then

3. **The reports request was made with status 200, and no admin request was made**

4. **Every export request went to the scoped endpoint**

5. **A check expecting a request the page never made fails, listing the requests it did see**

6. **The report lists the requests each pattern matched, with method, URL and status, and no bodies or headers**
