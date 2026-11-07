---
area: Hosted site
tags: [hub, sign-in]
owner: enocone
priority: high
stage: draft
---

# Teammates sign in with GitHub or a one-time email link

Flow documents are for the whole team, not just developers. Developers already
have GitHub accounts; product owners, testers and clients often don't. So
enoc.one offers both: GitHub for people who have it, and a one-time email
link for everyone else. Nobody manages a password.

```storyboard
- { from: hub-login, to: hub-link-sent, step: 2, on: { click: Email me a sign-in link } }
- { from: hub-link-sent, to: hub-project, step: 3, on: { click: Open sign-in link } }
```

```flowchart
flowchart LR
  1[Someone opens enoc.one] --> gh{GitHub account?}
  gh -->|yes| 4[Signs in with GitHub]
  gh -->|no| 2[Asks for an email link] --> 3[Opens the link once]
  4 --> in[Sees the projects they were invited to]
  3 --> in
  2 -.->|not invited| 5[Same message, nothing sent]
```

## Rules

- Only people invited to at least one project can sign in. Signing in never
  creates access on its own.
- An owner can also grant a project to a GitHub organisation or team
  (everyone in `acme/qa` can see `shop`). Membership is checked with GitHub at
  each sign-in, so people who leave the team lose access.
- An email link works once, expires after 15 minutes, and is stored only as a
  hash. Asking for links is rate limited per address and per IP.
- The page says the same thing whether or not the address is invited, so it
  can't be used to discover who is on a team.
- GitHub sign-in asks only for the user's identity and email (`read:user`,
  `user:email`), never repository access.
- Sessions last 30 days in an HttpOnly, Secure, SameSite=Lax cookie.

## Given

1. **Sam, a reviewer invited by email, opens enoc.one**
   ```screen
   screen: hub-login
   title: Sign in
   path: /login
   ```

## When

2. **Sam asks for a sign-in link**
   ```screen
   screen: hub-link-sent
   title: Link sent
   path: /login/sent
   ```

3. **Sam opens the link from the email**

## Then

4. **Sam sees the shop project and nothing else**
   ```screen
   screen: hub-project
   title: Project page
   path: /shop
   ```

5. **Opening the same link again shows that it has been used**

6. **Asking for a link for an address nobody invited shows the same message and sends nothing**

7. **Dev, a member of the GitHub team acme/qa that was granted shop, signs in with GitHub and sees shop**
