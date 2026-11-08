---
area: Hosted site
tags: [hub, visibility, open-source]
owner: enocone
priority: normal
stage: draft
---

# An owner makes an open-source project public, so anyone can read its flows and runs

Some projects are open source: their code is public, and their specs and
test runs can be too. Showing them to anyone, without an invitation, lets
users and contributors see what the product promises and whether it keeps
those promises. Other projects stay private. Every project starts private,
and an owner chooses to make it public, after being told what that shows.

```flowchart
flowchart LR
  1[A private project with runs and comments] --> 2[A visitor gets not found]
  1 --> 3[An owner makes it public, after a warning] --> 4[Anyone reads the project, runs and review pages]
  4 --> 5[Commenting and publishing still need an invitation]
  3 -.->|later| 7[Made private again: not found again]
```

## Rules

- Every project is private until an owner makes it public in the project's
  settings. It doesn't follow the GitHub repository's visibility: a public
  repository can still have runs its team would rather not publish.
- Before it becomes public, the owner is shown what that means: every run
  already published, with its screenshots, URLs and command output, becomes
  visible to anyone with the link.
- On a public project, anyone, signed in or not, can open the project page,
  its runs and reports, screenshots, and the review pages with their
  comments. Nothing on those pages can be changed by them.
- Commenting, resolving, publishing and managing members still need an
  invitation, as today. Signed-out visitors see "Sign in to comment" where
  the comment form would be, and the API refuses them.
- Public projects are reached by link; the home page doesn't list them.
- Reviewers' comments are shown to visitors on a public project: the owner
  chose to make its review public.
- For a public project, the MCP server's `review_comments` and the comments
  API work without signing in.
- A private project still answers "not found" to anyone who isn't a member,
  so its name isn't revealed.
- A public project shows a "Public" badge on its pages. An owner can make it
  private again at any time; from then on visitors get "not found".
- Public pages carry the same security headers as every other page, and
  can be found by search engines unless the owner turns that off.

## Given

1. **The shop project is private and has a published run with a comment on one flow**

## When

2. **A signed-out visitor opens the project, its run and its review page, and gets not found each time**

3. **Priya, an owner, makes the project public after reading the warning**

## Then

4. **The visitor can now read the project page, the run's report and the review page**

5. **The visitor sees "Sign in to comment" instead of the form, and the API refuses their comment and their publish**

6. **The project's pages show a Public badge**

7. **When Priya makes it private again, the visitor gets not found again**
