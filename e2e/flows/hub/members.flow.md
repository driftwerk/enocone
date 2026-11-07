---
area: Hosted site
tags: [hub, members]
owner: enocone
priority: normal
stage: draft
---

# Project owners invite the team, and everything is private by default

A team's flows describe unreleased features, so nothing on enocone.xyz is
public. An owner invites people by GitHub handle or email and gives each a
role.

```flowchart
flowchart LR
  1[Owner invites by handle or email] --> 2[Invitee signs in] --> 3[Sees only that project]
  4[Owner removes someone] --> 5[Their sessions and tokens stop working]
```

## Rules

- Roles: **owner** (manage members, link repositories), **member** (publish,
  comment, resolve, approve), **reviewer** (read, comment, approve).
- Every page and API call checks membership. Someone who is not a member gets
  a 404, not a 403, so project names don't leak.
- A project has at least one owner.
- Removing someone revokes their sessions and their publish tokens for that
  project straight away.

## Given

1. **Kingsley owns the shop project**

## When

2. **Kingsley invites sam@company.com as a reviewer**
   ```screen
   screen: hub-members
   title: Members
   path: /shop/members
   ```

## Then

3. **Sam can read and comment on shop but not publish to it**

4. **Someone who isn't a member gets “not found” for /shop**

5. **After Kingsley removes Priya, her publish token is refused**
